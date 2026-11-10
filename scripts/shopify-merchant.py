#!/usr/bin/env python3
"""Act as the merchant of the Shopify development store (what the store owner does in Shopify admin).

QuarantinePay never runs this: it only reads the merchant's order. Use it to fulfil or cancel the
orders QuarantinePay placed, from a terminal or a demo script.

  scripts/shopify-merchant.py list
  scripts/shopify-merchant.py fulfil '#1001' [TRACKING] [COMPANY]
  scripts/shopify-merchant.py cancel '#1001'

Reads SHOPIFY_DEV_STORE and SHOPIFY_CLIENT_ID/SECRET (or SHOPIFY_ADMIN_ACCESS_TOKEN) from the
environment or ./.env. Fulfilling needs the merchant-managed fulfillment order scopes.
"""
import json
import os
import re
import sys
import time
import urllib.parse
import urllib.request

VERSION = "2026-10"


def env():
    vals = dict(os.environ)
    if os.path.exists(".env"):
        for line in open(".env", encoding="utf-8"):
            m = re.match(r"^\s*([A-Z0-9_]+)\s*=\s*(.*)\s*$", line)
            if m and m.group(1) not in vals:
                vals[m.group(1)] = m.group(2).strip().strip('"').strip("'")
    return vals


E = env()
STORE = E.get("SHOPIFY_DEV_STORE", "")
if not re.fullmatch(r"[a-z0-9][a-z0-9-]*\.myshopify\.com", STORE):
    sys.exit("SHOPIFY_DEV_STORE must be <store>.myshopify.com")


def post(url, data, headers):
    req = urllib.request.Request(url, data=data, headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.load(r)


def token():
    if E.get("SHOPIFY_ADMIN_ACCESS_TOKEN"):
        return E["SHOPIFY_ADMIN_ACCESS_TOKEN"]
    body = urllib.parse.urlencode(
        {
            "grant_type": "client_credentials",
            "client_id": E.get("SHOPIFY_CLIENT_ID", ""),
            "client_secret": E.get("SHOPIFY_CLIENT_SECRET", ""),
        }
    ).encode()
    return post(
        f"https://{STORE}/admin/oauth/access_token",
        body,
        {"Content-Type": "application/x-www-form-urlencoded"},
    )["access_token"]


TOKEN = token()


def gql(query, variables=None):
    d = post(
        f"https://{STORE}/admin/api/{VERSION}/graphql.json",
        json.dumps({"query": query, "variables": variables or {}}).encode(),
        {"Content-Type": "application/json", "X-Shopify-Access-Token": TOKEN},
    )
    if d.get("errors"):
        sys.exit("Shopify: " + "; ".join(e.get("message", "") for e in d["errors"]))
    return d["data"]


def find(ref):
    if ref.startswith("gid://shopify/Order/"):
        return ref
    name = ref if ref.startswith("#") else f"#{ref}"
    # Order search is indexed a few seconds after creation; retry briefly.
    for _ in range(10):
        nodes = gql(
            "query($q:String!){ orders(first:1, query:$q){ nodes{ id } } }",
            {"q": f"name:{name} tag:quarantinepay"},
        )["orders"]["nodes"]
        if nodes:
            return nodes[0]["id"]
        time.sleep(2)
    sys.exit(f"no QuarantinePay order named {name}")


def errors(r, key="userErrors"):
    errs = r.get(key) or []
    if errs:
        sys.exit("Shopify: " + "; ".join(e["message"] for e in errs))


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else "list"
    if cmd == "list":
        for o in gql(
            "{ orders(first:10, reverse:true, query:\"tag:quarantinepay\"){ nodes{ name createdAt displayFinancialStatus displayFulfillmentStatus cancelledAt lineItems(first:1){ nodes{ title } } } } }"
        )["orders"]["nodes"]:
            item = (o["lineItems"]["nodes"] or [{"title": "?"}])[0]["title"]
            state = "CANCELLED" if o["cancelledAt"] else o["displayFulfillmentStatus"]
            print(f'{o["name"]:>7}  {o["displayFinancialStatus"]:<10} {state:<12} {item}  ({o["createdAt"]})')
    elif cmd == "fulfil" and len(sys.argv) >= 3:
        gid = find(sys.argv[2])
        number = sys.argv[3] if len(sys.argv) > 3 else "1Z999AA10123456784"
        company = sys.argv[4] if len(sys.argv) > 4 else "UPS"
        fos = gql(
            "query($id:ID!){ order(id:$id){ fulfillmentOrders(first:5){ nodes{ id status } } } }",
            {"id": gid},
        )["order"]["fulfillmentOrders"]["nodes"]
        open_fos = [f["id"] for f in fos if f["status"] in ("OPEN", "IN_PROGRESS")]
        if not open_fos:
            sys.exit("nothing left to fulfil on this order")
        r = gql(
            "mutation($f: FulfillmentInput!){ fulfillmentCreate(fulfillment:$f){ fulfillment{ id status } userErrors{ field message } } }",
            {
                "f": {
                    "lineItemsByFulfillmentOrder": [{"fulfillmentOrderId": i} for i in open_fos],
                    "trackingInfo": {"number": number, "company": company},
                    "notifyCustomer": False,
                }
            },
        )["fulfillmentCreate"]
        errors(r)
        print(f'Fulfilled {sys.argv[2]} with {company} {number}: {r["fulfillment"]["status"]}')
    elif cmd == "cancel" and len(sys.argv) >= 3:
        r = gql(
            "mutation($id:ID!){ orderCancel(orderId:$id, reason:INVENTORY, restock:true, notifyCustomer:false, refundMethod:{originalPaymentMethodsRefund:false}, staffNote:\"Merchant cancelled\"){ job{ id } orderCancelUserErrors{ field message } } }",
            {"id": find(sys.argv[2])},
        )["orderCancel"]
        errors(r, "orderCancelUserErrors")
        print(f"Cancelled {sys.argv[2]}")
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
