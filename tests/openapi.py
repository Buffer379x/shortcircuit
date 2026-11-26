"""Check that the API description and the native route table agree."""
import json
from pathlib import Path
import re
import unittest

ROOT = Path(__file__).resolve().parent.parent
DOCUMENT = json.loads((ROOT / "docs/openapi.json").read_text())
METHODS = {"get", "post", "patch", "delete", "put", "head", "options", "trace"}
ROUTE_START = re.compile(r'\.route\(\s*"(/api/v1/[^"]+)"\s*,\s*')
METHOD = re.compile(r'(?:^|\.)\s*(get|post|patch|delete|put|head|options|trace)\s*\(')


def deployed_methods():
    routes = {}
    for file in (ROOT / "src").rglob("*.rs"):
        source = file.read_text()
        for match in ROUTE_START.finditer(source):
            depth = 1
            end = match.end()
            while depth and end < len(source):
                depth += int(source[end] == "(") - int(source[end] == ")")
                end += 1
            methods = set(METHOD.findall(source[match.end():end - 1].strip()))
            if methods:
                routes.setdefault(match[1], set()).update(methods)
    return routes


def node_values(value):
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from node_values(child)
    elif isinstance(value, list):
        for child in value:
            yield from node_values(child)


class OpenApiContract(unittest.TestCase):
    def test_method_and_path_coverage(self):
        documented = {
            path: METHODS & entry.keys()
            for path, entry in DOCUMENT["paths"].items()
            if path.startswith("/api/v1/")
        }
        self.assertEqual(deployed_methods(), documented)

    def test_references_resolve_inside_the_document(self):
        for node in node_values(DOCUMENT):
            if "$ref" not in node:
                continue
            reference = node["$ref"]
            self.assertTrue(reference.startswith("#/"), reference)
            target = DOCUMENT
            for segment in reference[2:].split("/"):
                self.assertIn(segment, target, reference)
                target = target[segment]

    def test_each_operation_documents_a_success_and_any_path_parameter(self):
        schemes = DOCUMENT["components"]["securitySchemes"]
        for route, path in DOCUMENT["paths"].items():
            for method in METHODS & path.keys():
                operation = path[method]
                self.assertTrue(any(code.startswith("2") for code in operation["responses"]), (route, method))
                for group in operation.get("security", []):
                    self.assertFalse(set(group) - schemes.keys(), group)
                expected = set(re.findall(r"{([^}]+)}", route))
                parameters = path.get("parameters", []) + operation.get("parameters", [])
                declared = {param["name"] for param in parameters if param.get("in") == "path" and param["required"]}
                self.assertEqual(expected, declared, (route, method))


if __name__ == "__main__":
    unittest.main()
