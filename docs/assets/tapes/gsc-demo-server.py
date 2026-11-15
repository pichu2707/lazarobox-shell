#!/usr/bin/env python3
"""DEMO ONLY: a fake Search Console MCP server that returns INVENTED data.

It stands in for mcp-search-console in the README GIF (gsc-chat.tape) so the
chat can be shown querying Search Console without exposing a real account.
Nothing here talks to Google: every number belongs to the fictional property
sc-domain:neon-ramen.example and is made up.

It speaks MCP over stdio (JSON-RPC 2.0, one message per line) with only the
standard library: initialize, notifications/initialized, tools/list and
tools/call. Tool names and result shapes copy a few read-only tools of
mcp-search-console 0.4.2 (list_properties, get_performance_overview,
get_search_analytics).
"""

import json
import sys
from datetime import date, timedelta

SITE = "sc-domain:neon-ramen.example"
DAYS = 7

# Invented rows: query, clicks, impressions, average position.
QUERIES = [
    ("ramen neon madrid", 412, 5230, 2.1),
    ("ramen vegano cerca", 287, 6110, 4.3),
    ("neon ramen carta", 251, 1980, 1.4),
    ("mejor tonkotsu madrid", 176, 4890, 6.8),
    ("ramen picante delivery", 143, 3720, 5.2),
    ("gyozas caseras receta", 98, 7450, 9.7),
    ("neon ramen horario", 91, 860, 1.2),
    ("ramen sin gluten", 64, 2310, 8.5),
]

# Invented daily totals for the last DAYS days, oldest first.
DAILY = [
    (198, 4210, 7.4),
    (215, 4480, 7.1),
    (231, 4630, 6.9),
    (224, 4570, 7.0),
    (262, 5020, 6.6),
    (305, 5610, 6.2),
    (318, 5790, 6.0),
]

SITE_ARG = {
    "site_url": {
        "type": "string",
        "description": 'Exact GSC property URL from list_properties (e.g. "sc-domain:example.com").',
    }
}

TOOLS = [
    {
        "name": "list_properties",
        "description": "List all Google Search Console (GSC) properties the user has access to. "
        "Always call this first to get the exact site_url needed for other tools.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "get_performance_overview",
        "description": "Get a performance overview (total clicks, impressions, CTR, position "
        "and a daily trend) for a specific property.",
        "inputSchema": {
            "type": "object",
            "properties": {
                **SITE_ARG,
                "days": {"type": "integer", "description": "Number of days to look back (default: 28)"},
            },
            "required": ["site_url"],
        },
    },
    {
        "name": "get_search_analytics",
        "description": "Get search analytics data (top queries with clicks, impressions, CTR "
        "and position) for a specific property.",
        "inputSchema": {
            "type": "object",
            "properties": {
                **SITE_ARG,
                "days": {"type": "integer", "description": "Number of days to look back (default: 28)"},
                "dimensions": {"type": "string", "description": "Dimensions to group by (default: query)"},
                "row_limit": {"type": "integer", "description": "Number of rows to return (default: 20)"},
            },
            "required": ["site_url"],
        },
    },
]


def date_range(days):
    end = date.today()
    start = end - timedelta(days=days)
    return {"start": start.isoformat(), "end": end.isoformat(), "days": days}


def scaled(value, days):
    """Scales a weekly figure to the asked period, so other periods look plausible."""
    return round(value * days / DAYS)


def unknown_site(site):
    return f"Error: property {site!r} not found. Call list_properties for the exact site_url."


def list_properties(_args):
    return {"count": 1, "properties": [{"site_url": SITE, "permission_level": "siteOwner"}]}


def performance_overview(args):
    days = int(args.get("days") or 28)
    clicks = sum(scaled(c, days) for c, _, _ in DAILY)
    impressions = sum(scaled(i, days) for _, i, _ in DAILY)
    position = sum(p for _, _, p in DAILY) / len(DAILY)
    trend = []
    for offset, (c, i, p) in enumerate(DAILY):
        day = date.today() - timedelta(days=len(DAILY) - offset)
        trend.append({"date": day.isoformat(), "clicks": c, "impressions": i,
                      "ctr": round(c / i, 4), "position": p})
    return {
        "site_url": SITE,
        "date_range": date_range(days),
        "totals": {"clicks": clicks, "impressions": impressions,
                   "ctr": round(clicks / impressions, 4), "position": round(position, 1)},
        "daily_trend": trend[-min(days, len(trend)):],
    }


def search_analytics(args):
    days = int(args.get("days") or 28)
    limit = max(1, min(int(args.get("row_limit") or 20), len(QUERIES)))
    rows = [
        {"query": q, "clicks": scaled(c, days), "impressions": scaled(i, days),
         "ctr": round(c / i, 4), "position": p}
        for q, c, i, p in QUERIES[:limit]
    ]
    return {"site_url": SITE, "date_range": date_range(days), "dimensions": ["query"],
            "row_count": len(rows), "rows": rows}


HANDLERS = {
    "list_properties": list_properties,
    "get_performance_overview": performance_overview,
    "get_search_analytics": search_analytics,
}


def call_tool(params):
    name = params.get("name")
    args = params.get("arguments") or {}
    handler = HANDLERS.get(name)
    if handler is None:
        return {"content": [{"type": "text", "text": f"Unknown tool: {name}"}], "isError": True}
    site = args.get("site_url")
    if name != "list_properties" and site not in (SITE, "neon-ramen.example"):
        return {"content": [{"type": "text", "text": unknown_site(site)}], "isError": True}
    return {"content": [{"type": "text", "text": json.dumps(handler(args))}]}


def answer(request):
    method = request.get("method")
    params = request.get("params") or {}
    if method == "initialize":
        return {
            "protocolVersion": params.get("protocolVersion", "2025-06-18"),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "gsc-demo (invented data)", "version": "0.0.1"},
        }
    if method == "tools/list":
        return {"tools": TOOLS}
    if method == "tools/call":
        return call_tool(params)
    if method == "ping":
        return {}
    return None


def main():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            request = json.loads(line)
        except json.JSONDecodeError:
            continue
        if "id" not in request:
            continue  # a notification, such as notifications/initialized
        result = answer(request)
        if result is None:
            reply = {"jsonrpc": "2.0", "id": request["id"],
                     "error": {"code": -32601, "message": "Method not found"}}
        else:
            reply = {"jsonrpc": "2.0", "id": request["id"], "result": result}
        sys.stdout.write(json.dumps(reply) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
