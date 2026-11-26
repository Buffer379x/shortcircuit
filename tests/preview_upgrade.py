"""Exercise a link and account on a disposable preview upgrade volume."""

import http.client
import json
import os
import sys

ORIGIN = "http://localhost:8081"
USERNAME = "Preview operator"
PASSWORD = "synthetic-upgrade-password"
SLUG = "Upgrade/Guide"
TARGET = "https://example.test/guide?Signature=ABC%2f&part=two#original"


def request(port, method, path, payload=None, headers=None, expected=200):
    connection = http.client.HTTPConnection("localhost", port, timeout=5)
    actual_headers = headers or {}
    body = None
    if payload is not None:
        actual_headers = {**actual_headers, "Content-Type": "application/json"}
        body = json.dumps(payload)
    try:
        connection.request(method, path, body, actual_headers)
        response = connection.getresponse()
        contents = response.read()
        if response.status != expected:
            raise RuntimeError(
                f"{method} {path} returned {response.status}, expected {expected}"
            )
        return response, contents
    finally:
        connection.close()


def session(mode):
    credentials = {"username": USERNAME, "password": PASSWORD}
    if mode == "seed":
        credentials["token"] = os.environ["UPGRADE_SETUP_TOKEN"]
    operation, expected = ("setup", 201) if mode == "seed" else ("login", 200)
    response, contents = request(
        8081, "POST", f"/api/v1/auth/{operation}", credentials, {"Origin": ORIGIN}, expected
    )
    payload = json.loads(contents)
    assert payload["username"] == USERNAME
    return {
        "Origin": ORIGIN,
        "Cookie": response.getheader("Set-Cookie").split(";", 1)[0],
        "X-CSRF-Token": payload["csrf"],
    }


def main(mode):
    headers = session(mode)
    if mode == "seed":
        _, contents = request(
            8081,
            "POST",
            "/api/v1/links",
            {"host": "localhost", "slug": SLUG, "target": TARGET, "title": "Preview upgrade"},
            headers,
            201,
        )
        assert json.loads(contents)["slug"] == SLUG
    else:
        _, contents = request(
            8081, "POST", "/api/v1/links/search", {"query": "preview upgrade"}, headers
        )
        links = json.loads(contents)["items"]
        assert len(links) == 1
        link = links[0]
        assert link["slug"] == SLUG and link["target"] == TARGET
        assert len(link["selection_key"]) == 32
        _, updated = request(
            8081,
            "POST",
            "/api/v1/links/bulk",
            {
                "links": [{"id": link["id"], "key": link["selection_key"]}],
                "change": {"action": "add_tags", "tags": ["upgraded"]},
            },
            headers,
        )
        assert json.loads(updated)["items"][0]["tags"] == ["upgraded"]
    response, _ = request(8080, "GET", f"/{SLUG}", expected=302)
    assert response.getheader("Location") == TARGET
    print(f"Preview upgrade {mode} passed")


if __name__ == "__main__":
    if len(sys.argv) != 2 or sys.argv[1] not in {"seed", "verify"}:
        sys.exit("Usage: preview_upgrade.py seed|verify")
    main(sys.argv[1])
