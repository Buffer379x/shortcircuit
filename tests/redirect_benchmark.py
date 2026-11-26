#!/usr/bin/env python3
"""Measure ShortCircuit redirects without requesting their destinations."""

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import http.client
import json
import math
import re
from time import perf_counter_ns
from urllib.parse import urlsplit


def origin(value):
    address = urlsplit(value)
    if (
        address.scheme not in {"http", "https"}
        or not address.hostname
        or address.username is not None
        or address.path not in {"", "/"}
        or address.query
        or address.fragment
    ):
        raise argparse.ArgumentTypeError("provide a bare HTTP or HTTPS origin")
    try:
        address.port
    except ValueError as error:
        raise argparse.ArgumentTypeError("origin has an invalid port") from error
    return address


def slug(value):
    if len(value) > 200 or not re.fullmatch(r"[A-Za-z0-9_-]+(?:/[A-Za-z0-9_-]+)*", value):
        raise argparse.ArgumentTypeError("provide an exact ShortCircuit slug")
    return value


def positive(value):
    if value.isascii() and value.isdecimal():
        count = int(value)
        if 1 <= count <= 1_000_000:
            return count
    raise argparse.ArgumentTypeError("provide an integer from 1 to 1000000")


def run_worker(address, slugs, expected, count, offset, stride):
    connection_class = (
        http.client.HTTPSConnection if address.scheme == "https" else http.client.HTTPConnection
    )
    connection = connection_class(address.hostname, address.port, timeout=5)
    latencies = []
    errors = Counter()
    try:
        for index in range(offset, count, stride):
            target = "/" + slugs[index % len(slugs)]
            start = perf_counter_ns()
            try:
                connection.request("GET", target)
                response = connection.getresponse()
                # Redirects have small or empty bodies. Read only a bounded amount.
                response.read(4097)
                elapsed = perf_counter_ns() - start
                if response.status == expected and response.getheader("location"):
                    latencies.append(elapsed)
                else:
                    errors["http_" + str(response.status)] += 1
                if not response.isclosed():
                    connection.close()
            except (OSError, http.client.HTTPException) as error:
                errors[type(error).__name__] += 1
                connection.close()
    finally:
        connection.close()
    return latencies, errors


def run_phase(address, slugs, expected, count, concurrency):
    lanes = min(count, concurrency)
    start = perf_counter_ns()
    latencies = []
    errors = Counter()
    with ThreadPoolExecutor(max_workers=lanes) as workers:
        tasks = [
            workers.submit(run_worker, address, slugs, expected, count, lane, lanes)
            for lane in range(lanes)
        ]
        for task in tasks:
            successes, failures = task.result()
            latencies.extend(successes)
            errors.update(failures)
    return latencies, errors, (perf_counter_ns() - start) / 1_000_000_000


def percentile(ordered, fraction):
    if not ordered:
        return None
    rank = math.ceil(len(ordered) * fraction) - 1
    return round(ordered[rank] / 1_000_000, 3)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", type=origin, default="http://127.0.0.1:8080")
    parser.add_argument("--slug", type=slug, action="append", required=True)
    parser.add_argument("--requests", type=positive, default=10000)
    parser.add_argument("--warmup", type=positive, default=200)
    parser.add_argument("--concurrency", type=positive, default=16)
    parser.add_argument("--status", type=int, choices=(301, 302, 307, 308), default=302)
    args = parser.parse_args()
    if args.concurrency > 128:
        parser.error("concurrency must be at most 128")
    _, warmup_errors, _ = run_phase(
        args.base_url, args.slug, args.status, args.warmup, args.concurrency
    )
    if warmup_errors:
        print(json.dumps({"warmup_errors": warmup_errors}, indent=2))
        return 1
    latencies, errors, seconds = run_phase(
        args.base_url, args.slug, args.status, args.requests, args.concurrency
    )
    latencies.sort()
    print(json.dumps({
        "requests": args.requests,
        "warmup": args.warmup,
        "concurrency": min(args.concurrency, args.requests),
        "distinct_slugs": len(set(args.slug)),
        "successful": len(latencies),
        "errors": errors,
        "seconds": round(seconds, 4),
        "successful_requests_per_second": round(len(latencies) / seconds, 2),
        "p50_ms": percentile(latencies, 0.50),
        "p95_ms": percentile(latencies, 0.95),
        "p99_ms": percentile(latencies, 0.99),
    }, indent=2))
    return int(bool(errors))


if __name__ == "__main__":
    raise SystemExit(main())
