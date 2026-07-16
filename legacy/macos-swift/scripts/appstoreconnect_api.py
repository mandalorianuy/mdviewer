#!/usr/bin/env python3

import argparse
import base64
import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request


def b64url(data: bytes) -> str:
    return base64.urlsafe_b64encode(data).rstrip(b"=").decode("ascii")


def der_to_jose_signature(der: bytes) -> bytes:
    if len(der) < 8 or der[0] != 0x30:
        raise ValueError("Unsupported DER signature format")

    idx = 2
    if der[1] & 0x80:
        length_len = der[1] & 0x7F
        idx = 2 + length_len

    if der[idx] != 0x02:
        raise ValueError("Missing R integer in DER signature")
    r_len = der[idx + 1]
    r = der[idx + 2:idx + 2 + r_len]
    idx = idx + 2 + r_len

    if der[idx] != 0x02:
        raise ValueError("Missing S integer in DER signature")
    s_len = der[idx + 1]
    s = der[idx + 2:idx + 2 + s_len]

    r = r.lstrip(b"\x00").rjust(32, b"\x00")
    s = s.lstrip(b"\x00").rjust(32, b"\x00")
    return r + s


def generate_jwt(key_id: str, issuer_id: str, private_key_path: str) -> str:
    header = {"alg": "ES256", "kid": key_id, "typ": "JWT"}
    now = int(time.time())
    payload = {
        "iss": issuer_id,
        "aud": "appstoreconnect-v1",
        "exp": now + 1200,
    }

    signing_input = ".".join(
        [
            b64url(json.dumps(header, separators=(",", ":")).encode("utf-8")),
            b64url(json.dumps(payload, separators=(",", ":")).encode("utf-8")),
        ]
    )

    proc = subprocess.run(
        [
            "openssl",
            "dgst",
            "-sha256",
            "-sign",
            private_key_path,
        ],
        input=signing_input.encode("utf-8"),
        capture_output=True,
        check=True,
    )
    signature = der_to_jose_signature(proc.stdout)
    return signing_input + "." + b64url(signature)


def request_api(method: str, path: str, token: str, body=None):
    url = "https://api.appstoreconnect.apple.com" + path
    data = None
    headers = {
        "Authorization": f"Bearer {token}",
        "Accept": "application/json",
    }

    if body is not None:
        data = json.dumps(body).encode("utf-8")
        headers["Content-Type"] = "application/json"

    req = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(req) as resp:
            raw = resp.read().decode("utf-8")
            return resp.status, json.loads(raw) if raw else {}
    except urllib.error.HTTPError as err:
        raw = err.read().decode("utf-8")
        payload = json.loads(raw) if raw else {}
        return err.code, payload


def cmd_raw_get(args):
    token = generate_jwt(args.key_id, args.issuer_id, args.private_key)
    status, payload = request_api("GET", args.path, token)
    print(json.dumps({"status": status, "payload": payload}, indent=2))
    return 0 if status < 300 else 1


def cmd_raw_patch(args):
    token = generate_jwt(args.key_id, args.issuer_id, args.private_key)
    body = json.loads(args.body)
    status, payload = request_api("PATCH", args.path, token, body=body)
    print(json.dumps({"status": status, "payload": payload}, indent=2))
    return 0 if status < 300 else 1


def cmd_raw_post(args):
    token = generate_jwt(args.key_id, args.issuer_id, args.private_key)
    body = json.loads(args.body)
    status, payload = request_api("POST", args.path, token, body=body)
    print(json.dumps({"status": status, "payload": payload}, indent=2))
    return 0 if status < 300 else 1


def cmd_get_bundle_id(args):
    token = generate_jwt(args.key_id, args.issuer_id, args.private_key)
    query = urllib.parse.urlencode({"filter[identifier]": args.identifier})
    status, payload = request_api("GET", f"/v1/bundleIds?{query}", token)
    print(json.dumps({"status": status, "payload": payload}, indent=2))
    return 0 if status < 300 else 1


def cmd_list_apps(args):
    token = generate_jwt(args.key_id, args.issuer_id, args.private_key)
    query = urllib.parse.urlencode({"filter[bundleId]": args.bundle_id})
    status, payload = request_api("GET", f"/v1/apps?{query}", token)
    print(json.dumps({"status": status, "payload": payload}, indent=2))
    return 0 if status < 300 else 1


def cmd_create_app(args):
    token = generate_jwt(args.key_id, args.issuer_id, args.private_key)
    body = {
        "data": {
            "type": "apps",
            "attributes": {
                "name": args.name,
                "primaryLocale": args.primary_locale,
                "sku": args.sku,
            },
            "relationships": {
                "bundleId": {
                    "data": {
                        "type": "bundleIds",
                        "id": args.bundle_id_resource_id,
                    }
                }
            },
        }
    }
    status, payload = request_api("POST", "/v1/apps", token, body=body)
    print(json.dumps({"status": status, "payload": payload}, indent=2))
    return 0 if status < 300 else 1


def cmd_create_bundle_id(args):
    token = generate_jwt(args.key_id, args.issuer_id, args.private_key)
    body = {
        "data": {
            "type": "bundleIds",
            "attributes": {
                "identifier": args.identifier,
                "name": args.name,
                "platform": args.platform,
            },
        }
    }
    status, payload = request_api("POST", "/v1/bundleIds", token, body=body)
    print(json.dumps({"status": status, "payload": payload}, indent=2))
    return 0 if status < 300 else 1


def build_parser():
    parser = argparse.ArgumentParser()
    parser.add_argument("--key-id", default=os.environ.get("ASC_KEY_ID"))
    parser.add_argument("--issuer-id", default=os.environ.get("ASC_ISSUER_ID"))
    parser.add_argument(
        "--private-key",
        default=os.environ.get("ASC_PRIVATE_KEY"),
    )

    subparsers = parser.add_subparsers(dest="command", required=True)

    bundle = subparsers.add_parser("get-bundle-id")
    bundle.add_argument("--identifier", required=True)
    bundle.set_defaults(func=cmd_get_bundle_id)

    create_bundle = subparsers.add_parser("create-bundle-id")
    create_bundle.add_argument("--identifier", required=True)
    create_bundle.add_argument("--name", required=True)
    create_bundle.add_argument("--platform", default="MAC_OS")
    create_bundle.set_defaults(func=cmd_create_bundle_id)

    apps = subparsers.add_parser("list-apps")
    apps.add_argument("--bundle-id", required=True)
    apps.set_defaults(func=cmd_list_apps)

    raw_get = subparsers.add_parser("raw-get")
    raw_get.add_argument("--path", required=True)
    raw_get.set_defaults(func=cmd_raw_get)

    raw_patch = subparsers.add_parser("raw-patch")
    raw_patch.add_argument("--path", required=True)
    raw_patch.add_argument("--body", required=True)
    raw_patch.set_defaults(func=cmd_raw_patch)

    raw_post = subparsers.add_parser("raw-post")
    raw_post.add_argument("--path", required=True)
    raw_post.add_argument("--body", required=True)
    raw_post.set_defaults(func=cmd_raw_post)

    create = subparsers.add_parser("create-app")
    create.add_argument("--name", required=True)
    create.add_argument("--sku", required=True)
    create.add_argument("--primary-locale", required=True)
    create.add_argument("--bundle-id-resource-id", required=True)
    create.set_defaults(func=cmd_create_app)

    return parser


def main():
    parser = build_parser()
    args = parser.parse_args()
    for option in ("key_id", "issuer_id", "private_key"):
        if not getattr(args, option):
            parser.error(
                f"--{option.replace('_', '-')} or its ASC environment variable is required"
            )
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
