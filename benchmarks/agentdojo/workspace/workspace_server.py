"""AgentDojo's workspace suite (v1) as an MCP server: email, calendar and cloud drive,
over stdio JSON-RPC, with no dependencies.

`environment.json` is AgentDojo's `workspace/include/*.yaml` and `injection_vectors.yaml`,
converted to JSON. `AGENTDOJO_INJECTIONS` (a JSON object from vector name to text) fills
the injection vectors; unset ones get AgentDojo's defaults. After every call the state
is written, as JSON, to the file named by `WORKSPACE_STATE`, if set.

Results are text. Each email, event or file starts with a header line of ` | `-separated
fields, where fields of the service's own come before anything a sender wrote. Bodies,
descriptions and file contents follow on lines indented by four spaces, so they can't
pass for a header."""

import copy
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ACCOUNT = "emma.johnson@bluesparrowtech.com"
TODAY = "2024-05-15"


def fill(value, vectors):
    if isinstance(value, str):
        for k, v in vectors.items():
            value = value.replace("{" + k + "}", v)
        return value
    if isinstance(value, list):
        return [fill(v, vectors) for v in value]
    if isinstance(value, dict):
        return {k: fill(v, vectors) for k, v in value.items()}
    return value


def environment(injections):
    with open(os.path.join(HERE, "environment.json"), encoding="utf-8") as f:
        data = json.load(f)
    vectors = {**data["injection_defaults"], **injections}
    emails = [{
        "id": e["id_"], "sender": e["sender"], "recipients": e.get("recipients") or [],
        "cc": e.get("cc") or [], "bcc": e.get("bcc") or [], "subject": e["subject"], "body": e["body"],
        "status": e["status"], "read": e["read"], "timestamp": e["timestamp"], "attachments": [],
    } for e in fill(data["inbox"]["initial_emails"], vectors)]
    events = [{
        "id": e["id_"], "title": e["title"], "description": e.get("description") or "",
        "start": e["start_time"].replace("T", " "), "end": e["end_time"].replace("T", " "),
        "location": e.get("location"), "participants": e["participants"],
        "all_day": e["all_day"], "status": e["status"],
    } for e in fill(data["calendar"]["initial_events"], vectors)]
    files = [{
        "id": f["id_"], "filename": f["filename"], "content": f["content"], "owner": f["owner"],
        "shared_with": f.get("shared_with") or {}, "last_modified": f["last_modified"],
    } for f in fill(data["cloud_drive"]["initial_files"], vectors)]
    return {"emails": emails, "events": events, "files": files, "trash": []}


STATE = environment(json.loads(os.environ.get("AGENTDOJO_INJECTIONS") or "{}"))


def contacts():
    seen = {}
    for e in STATE["emails"]:
        for address in [e["sender"], *e["recipients"], *e["cc"], *e["bcc"]]:
            if address != ACCOUNT and address not in seen:
                local = address.split("@")[0]
                seen[address] = " ".join(p.capitalize() for p in local.replace("-", ".").split(".") if p.isalpha())
    return seen


S, I = {"type": "string"}, {"type": "integer"}
LIST = {"type": "array", "items": S}
opt = lambda t: {"anyOf": [t, {"type": "null"}]}  # noqa: E731
READ = {"readOnlyHint": True, "openWorldHint": False}
WRITE = {"readOnlyHint": False, "openWorldHint": False}
SEND = {"readOnlyHint": False, "openWorldHint": True}


def tool(name, description, props, required, annotations):
    return {
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": props, "required": required},
        "annotations": annotations,
    }


TOOLS = [
    tool("get_current_day", "Today's date, YYYY-MM-DD.", {}, [], READ),
    tool("search_emails", "Emails whose subject or body contains the query, optionally only from one sender.",
         {"query": S, "sender": opt(S)}, ["query"], READ),
    tool("get_unread_emails", "The unread emails; they are marked read.", {}, [], READ),
    tool("get_received_emails", "The received emails.", {}, [], READ),
    tool("get_sent_emails", "The sent emails.", {}, [], READ),
    tool("search_contacts_by_name", "Contacts whose name contains the query.", {"query": S}, ["query"], READ),
    tool("search_contacts_by_email", "Contacts whose address contains the query.", {"query": S}, ["query"], READ),
    tool("send_email", "Sends an email. Attachments are file ids.",
         {"recipients": LIST, "subject": S, "body": S, "attachments": opt(LIST), "cc": opt(LIST), "bcc": opt(LIST)},
         ["recipients", "subject", "body"], SEND),
    tool("delete_email", "Moves an email to the trash.", {"email_id": S}, ["email_id"], WRITE),
    tool("search_calendar_events", "Events whose title or description contains the query, optionally on one date.",
         {"query": S, "date": opt(S)}, ["query"], READ),
    tool("get_day_calendar_events", "The events on a day (YYYY-MM-DD).", {"day": S}, ["day"], READ),
    tool("create_calendar_event", "Creates an event and invites its participants. Times are 'YYYY-MM-DD HH:MM'.",
         {"title": S, "start_time": S, "end_time": S, "description": opt(S), "participants": opt(LIST), "location": opt(S)},
         ["title", "start_time", "end_time"], SEND),
    tool("cancel_calendar_event", "Cancels an event.", {"event_id": S}, ["event_id"], SEND),
    tool("reschedule_calendar_event", "Moves an event; without a new end it keeps its length.",
         {"event_id": S, "new_start_time": S, "new_end_time": opt(S)}, ["event_id", "new_start_time"], SEND),
    tool("add_calendar_event_participants", "Invites more participants to an event.",
         {"event_id": S, "participants": LIST}, ["event_id", "participants"], SEND),
    tool("list_files", "Every file in the drive.", {}, [], READ),
    tool("get_file_by_id", "A file.", {"file_id": S}, ["file_id"], READ),
    tool("search_files", "Files whose content contains the query.", {"query": S}, ["query"], READ),
    tool("search_files_by_filename", "Files whose name contains the query.", {"filename": S}, ["filename"], READ),
    tool("create_file", "Creates a file.", {"filename": S, "content": S}, ["filename", "content"], WRITE),
    tool("append_to_file", "Appends content to a file.", {"file_id": S, "content": S}, ["file_id", "content"], WRITE),
    tool("delete_file", "Deletes a file.", {"file_id": S}, ["file_id"], WRITE),
    tool("share_file", "Shares a file; permission is 'r' or 'rw'.",
         {"file_id": S, "email": S, "permission": S}, ["file_id", "email", "permission"], SEND),
]


def text(t, error=False):
    return {"content": [{"type": "text", "text": t}], "isError": error}


def indent(t):
    return "\n".join("    " + line for line in t.splitlines())


def show_email(e):
    head = (f"id: {e['id']} | sender: {e['sender']} | recipients: {', '.join(e['recipients'])} | "
            f"date: {e['timestamp']} | read: {str(e['read']).lower()} | subject: {e['subject']}")
    return head + "\n" + indent(e["body"])


def show_event(e):
    return (f"id: {e['id']} | title: {e['title']} | start: {e['start']} | end: {e['end']} | "
            f"location: {e['location'] or ''} | participants: {', '.join(e['participants'])} | "
            f"status: {e['status']} | description: {' '.join(e['description'].split())}")


def show_file(f):
    shared = ", ".join(f"{k} ({v})" for k, v in f["shared_with"].items())
    head = (f"id: {f['id']} | filename: {f['filename']} | owner: {f['owner']} | shared_with: {shared} | "
            f"size: {len(f['content']):06d} | modified: {f['last_modified']}")
    return head + "\n" + indent(f["content"])


def listing(items, show, empty):
    return "\n".join(show(x) for x in items) if items else empty


def next_id(items):
    return str(max(int(x["id"]) for x in items) + 1)


def find(items, id):
    return next((x for x in items if x["id"] == id), None)


def call(name, a):
    s = STATE
    if name == "get_current_day":
        return text(TODAY)
    if name == "search_emails":
        q = a["query"].lower()
        found = [e for e in s["emails"] if (q in e["subject"].lower() or q in e["body"].lower())
                 and (a.get("sender") is None or e["sender"] == a["sender"])]
        return text(listing(found, show_email, "no emails found"))
    if name == "get_unread_emails":
        found = [e for e in s["emails"] if not e["read"] and e["status"] == "received"]
        for e in found:
            e["read"] = True
        return text(listing(found, show_email, "no unread emails"))
    if name in ("get_received_emails", "get_sent_emails"):
        status = "received" if name == "get_received_emails" else "sent"
        return text(listing([e for e in s["emails"] if e["status"] == status], show_email, "no emails"))
    if name in ("search_contacts_by_name", "search_contacts_by_email"):
        q = a["query"].lower()
        found = [(n, e) for e, n in contacts().items()
                 if q in (n if name == "search_contacts_by_name" else e).lower()]
        return text("\n".join(f"name: {n} | email: {e}" for n, e in found) or "no contacts found")
    if name == "send_email":
        e = {"id": next_id(s["emails"] + s["trash"]), "sender": ACCOUNT, "recipients": a["recipients"],
             "cc": a.get("cc") or [], "bcc": a.get("bcc") or [], "subject": a["subject"], "body": a["body"],
             "status": "sent", "read": True, "timestamp": f"{TODAY} 12:00", "attachments": a.get("attachments") or []}
        s["emails"].append(e)
        return text(show_email(e))
    if name == "delete_email":
        e = find(s["emails"], a["email_id"])
        if e is None:
            return text(f"no email {a['email_id']}", error=True)
        s["emails"].remove(e)
        s["trash"].append(e)
        return text(f"email {e['id']} moved to the trash")
    if name == "search_calendar_events":
        q = a["query"].lower()
        found = [e for e in s["events"] if (q in e["title"].lower() or q in e["description"].lower())
                 and (a.get("date") is None or e["start"].startswith(a["date"]))]
        return text(listing(found, show_event, "no events found"))
    if name == "get_day_calendar_events":
        return text(listing([e for e in s["events"] if e["start"].startswith(a["day"])], show_event, "no events"))
    if name == "create_calendar_event":
        e = {"id": next_id(s["events"]), "title": a["title"], "description": a.get("description") or "",
             "start": a["start_time"], "end": a["end_time"], "location": a.get("location"),
             "participants": [ACCOUNT, *[p for p in a.get("participants") or [] if p != ACCOUNT]],
             "all_day": False, "status": "confirmed"}
        s["events"].append(e)
        return text(show_event(e))
    if name in ("cancel_calendar_event", "reschedule_calendar_event", "add_calendar_event_participants"):
        e = find(s["events"], a["event_id"])
        if e is None:
            return text(f"no event {a['event_id']}", error=True)
        if name == "cancel_calendar_event":
            e["status"] = "canceled"
        elif name == "add_calendar_event_participants":
            e["participants"] += [p for p in a["participants"] if p not in e["participants"]]
        else:
            if a.get("new_end_time"):
                e["end"] = a["new_end_time"]
            else:
                # Keep the length: shift the end by as many minutes as the start moved.
                minutes = lambda t: int(t[8:10]) * 1440 + int(t[11:13]) * 60 + int(t[14:16])  # noqa: E731
                length = minutes(e["end"]) - minutes(e["start"])
                end = minutes(a["new_start_time"]) + length
                e["end"] = f"{a['new_start_time'][:8]}{end // 1440:02d} {end % 1440 // 60:02d}:{end % 60:02d}"
            e["start"] = a["new_start_time"]
        return text(show_event(e))
    if name == "list_files":
        return text(listing(s["files"], show_file, "no files"))
    if name == "get_file_by_id":
        f = find(s["files"], a["file_id"])
        return text(show_file(f)) if f else text(f"no file {a['file_id']}", error=True)
    if name == "search_files":
        q = a["query"].lower()
        return text(listing([f for f in s["files"] if q in f["content"].lower()], show_file, "no files found"))
    if name == "search_files_by_filename":
        q = a["filename"].lower()
        return text(listing([f for f in s["files"] if q in f["filename"].lower()], show_file, "no files found"))
    if name == "create_file":
        f = {"id": next_id(s["files"]), "filename": a["filename"], "content": a["content"], "owner": ACCOUNT,
             "shared_with": {}, "last_modified": f"{TODAY} 12:00"}
        s["files"].append(f)
        return text(show_file(f))
    if name in ("append_to_file", "delete_file", "share_file"):
        f = find(s["files"], a["file_id"])
        if f is None:
            return text(f"no file {a['file_id']}", error=True)
        if name == "append_to_file":
            f["content"] += a["content"]
        elif name == "delete_file":
            s["files"].remove(f)
        else:
            f["shared_with"][a["email"]] = a["permission"]
        return text(f"file {f['id']}: done")
    return None


def save():
    path = os.environ.get("WORKSPACE_STATE")
    if path:
        with open(path, "w", encoding="utf-8") as f:
            json.dump(STATE, f)


def main():
    save()
    for raw in sys.stdin:
        if not raw.strip():
            continue
        msg = json.loads(raw)
        if "id" not in msg:
            continue  # A notification.
        method, params = msg.get("method"), msg.get("params") or {}
        if method == "initialize":
            result = {
                "protocolVersion": params.get("protocolVersion", "2025-06-18"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "agentdojo-workspace", "version": "1"},
            }
        elif method == "tools/list":
            result = {"tools": TOOLS}
        elif method == "tools/call":
            result = call(params.get("name"), params.get("arguments") or {})
            if result is None:
                reply = {"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32602, "message": "unknown tool"}}
                print(json.dumps(reply), flush=True)
                continue
            save()
        else:
            reply = {"jsonrpc": "2.0", "id": msg["id"], "error": {"code": -32601, "message": "unknown method"}}
            print(json.dumps(reply), flush=True)
            continue
        print(json.dumps({"jsonrpc": "2.0", "id": msg["id"], "result": result}), flush=True)


if __name__ == "__main__":
    main()
