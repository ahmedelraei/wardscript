"""AgentDojo's travel suite (v1) as an MCP server: hotels, restaurants, car rental,
flights, reservations, calendar and email, over stdio JSON-RPC, with no dependencies.

`environment.json` is AgentDojo's `travel/environment.yaml` and `injection_vectors.yaml`,
converted to JSON. `AGENTDOJO_INJECTIONS` (a JSON object from vector name to text) fills
the injection vectors; unset ones get AgentDojo's defaults. After every call the state
is written, as JSON, to the file named by `TRAVEL_STATE`, if set.

Results are text. Each hotel, restaurant, company, flight or event is a header line of
` | `-separated fields that the service keeps; reviews, which anyone can write, follow
on lines indented by four spaces, so they can't pass for a header."""

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
    data = fill(data, {**data["injection_defaults"], **injections})
    emails = [{
        "id": e["id_"], "sender": e["sender"], "recipients": e.get("recipients") or [],
        "cc": e.get("cc") or [], "bcc": e.get("bcc") or [], "subject": e["subject"], "body": e["body"],
        "status": e["status"], "timestamp": e["timestamp"],
    } for e in data["inbox"]["initial_emails"]]
    events = [{
        "id": e["id_"], "title": e["title"], "description": e.get("description") or "",
        "start": e["start_time"].replace("T", " "), "end": e["end_time"].replace("T", " "),
        "location": e.get("location"), "participants": e["participants"], "status": e["status"],
    } for e in data["calendar"]["initial_events"]]
    return {
        "user": data["user"],
        "hotels": data["hotels"]["hotel_list"],
        "restaurants": data["restaurants"]["restaurant_list"],
        "companies": data["car_rental"]["company_list"],
        "flights": data["flights"]["flight_list"],
        "reservation": copy.deepcopy(data["reservation"]),
        "events": events,
        "emails": emails,
    }


STATE = environment(json.loads(os.environ.get("AGENTDOJO_INJECTIONS") or "{}"))

S, I = {"type": "string"}, {"type": "integer"}
LIST = {"type": "array", "items": S}
opt = lambda t: {"anyOf": [t, {"type": "null"}]}  # noqa: E731
READ = {"readOnlyHint": True, "openWorldHint": False}
SEND = {"readOnlyHint": False, "openWorldHint": True}


def tool(name, description, props, required, annotations):
    return {
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": props, "required": required},
        "annotations": annotations,
    }


def names_tool(name, description, param="names"):
    return tool(name, description, {param: LIST}, [param], READ)


TOOLS = [
    tool("get_user_information", "The user's name, contact details, ID, passport and bank details.", {}, [], READ),
    tool("get_all_hotels_in_city", "The hotels in a city.", {"city": S}, ["city"], READ),
    names_tool("get_hotels_prices", "The price range per night of each hotel."),
    tool("get_hotels_address", "The address of a hotel.", {"hotel_name": S}, ["hotel_name"], READ),
    names_tool("get_rating_reviews_for_hotels", "The rating and reviews of each hotel."),
    tool("get_all_restaurants_in_city", "The restaurants in a city.", {"city": S}, ["city"], READ),
    names_tool("get_restaurants_address", "The address of each restaurant."),
    names_tool("get_rating_reviews_for_restaurants", "The rating and reviews of each restaurant."),
    names_tool("get_cuisine_type_for_restaurants", "The cuisine of each restaurant."),
    names_tool("get_dietary_restrictions_for_all_restaurants", "The dietary options of each restaurant."),
    names_tool("get_contact_information_for_restaurants", "The contact information of each restaurant."),
    names_tool("get_price_for_restaurants", "The price per person at each restaurant."),
    names_tool("check_restaurant_opening_hours", "The opening hours of each restaurant."),
    tool("get_all_car_rental_companies_in_city", "The car rental companies in a city.", {"city": S}, ["city"], READ),
    names_tool("get_car_types_available", "The car types each company rents."),
    names_tool("get_rating_reviews_for_car_rental", "The rating and reviews of each company."),
    names_tool("get_car_rental_address", "The address of each company."),
    names_tool("get_car_fuel_options", "The fuel options of each company."),
    names_tool("get_car_price_per_day", "The price per day at each company."),
    tool("get_flight_information", "The flights between two cities.",
         {"departure_city": S, "arrival_city": S}, ["departure_city", "arrival_city"], READ),
    tool("reserve_hotel", "Reserves a hotel. Days are YYYY-MM-DD.",
         {"hotel": S, "start_day": S, "end_day": S}, ["hotel", "start_day", "end_day"], SEND),
    tool("reserve_restaurant", "Reserves a table for two hours from 'YYYY-MM-DD HH:MM'.",
         {"restaurant": S, "start_time": S}, ["restaurant", "start_time"], SEND),
    tool("reserve_car_rental", "Reserves a car. Times are 'YYYY-MM-DD HH:MM'.",
         {"company": S, "start_time": S, "end_time": S}, ["company", "start_time", "end_time"], SEND),
    tool("search_calendar_events", "Events whose title or description contains the query, optionally on one date.",
         {"query": S, "date": opt(S)}, ["query"], READ),
    tool("get_day_calendar_events", "The events on a day (YYYY-MM-DD).", {"day": S}, ["day"], READ),
    tool("create_calendar_event", "Creates an event and invites its participants. Times are 'YYYY-MM-DD HH:MM'.",
         {"title": S, "start_time": S, "end_time": S, "description": opt(S), "participants": opt(LIST), "location": opt(S)},
         ["title", "start_time", "end_time"], SEND),
    tool("cancel_calendar_event", "Cancels an event.", {"event_id": S}, ["event_id"], SEND),
    tool("send_email", "Sends an email.",
         {"recipients": LIST, "subject": S, "body": S, "attachments": opt(LIST), "cc": opt(LIST), "bcc": opt(LIST)},
         ["recipients", "subject", "body"], SEND),
]


def text(t, error=False):
    return {"content": [{"type": "text", "text": t}], "isError": error}


def indent(lines):
    return "".join(f"\n    {' '.join(line.split())}" for line in lines)


def show(item, fields, reviews=False):
    head = " | ".join(f"{k}: {item[k]}" for k in fields)
    return head + (indent(item["reviews"]) if reviews else "")


def named(items, names, fields, reviews=False):
    found = [show(x, fields, reviews) for x in items if x["name"] in names]
    return text("\n".join(found) or "nothing found")


def in_city(items, city):
    return text("\n".join(f"name: {x['name']}" for x in items if x["city"] == city) or "nothing found")


def show_event(e):
    return (f"id: {e['id']} | title: {e['title']} | start: {e['start']} | end: {e['end']} | "
            f"location: {e['location'] or ''} | participants: {', '.join(e['participants'])} | "
            f"status: {e['status']} | description: {' '.join(e['description'].split())}")


def next_id(items):
    return str(max(int(x["id"]) for x in items) + 1)


def reserve(kind, title, start, end):
    STATE["reservation"] = {
        "reservation_type": kind, "title": title, "start_time": start, "end_time": end,
        "contact_information": STATE["user"]["phone_number"],
    }


def call(name, a):
    s = STATE
    hotels, restaurants, companies = s["hotels"], s["restaurants"], s["companies"]
    if name == "get_user_information":
        u = s["user"]
        return text(" | ".join(f"{k}: {v}" for k, v in u.items()))
    if name == "get_all_hotels_in_city":
        return in_city(hotels, a["city"])
    if name == "get_hotels_prices":
        return named(hotels, a["names"], ("name", "price_min", "price_max"))
    if name == "get_hotels_address":
        return named(hotels, [a["hotel_name"]], ("name", "address"))
    if name == "get_rating_reviews_for_hotels":
        return named(hotels, a["names"], ("name", "rating"), reviews=True)
    if name == "get_all_restaurants_in_city":
        return in_city(restaurants, a["city"])
    fields = {
        "get_restaurants_address": ("address",),
        "get_rating_reviews_for_restaurants": ("rating",),
        "get_cuisine_type_for_restaurants": ("cuisine_type",),
        "get_dietary_restrictions_for_all_restaurants": ("dietary_restrictions",),
        "get_contact_information_for_restaurants": ("contact_information",),
        "get_price_for_restaurants": ("price_per_person",),
        "check_restaurant_opening_hours": ("operating_hours",),
    }
    if name in fields:
        return named(restaurants, a["names"], ("name", *fields[name]), reviews=name.startswith("get_rating"))
    if name == "get_all_car_rental_companies_in_city":
        return in_city(companies, a["city"])
    fields = {
        "get_car_types_available": ("car_types_available",),
        "get_rating_reviews_for_car_rental": ("rating",),
        "get_car_rental_address": ("address",),
        "get_car_fuel_options": ("fuel_options",),
        "get_car_price_per_day": ("price_per_day",),
    }
    if name in fields:
        lists = [{**c, **{k: ", ".join(v) for k, v in c.items() if isinstance(v, list) and k != "reviews"}} for c in companies]
        return named(lists, a["names"], ("name", *fields[name]), reviews=name.startswith("get_rating"))
    if name == "get_flight_information":
        found = [f for f in s["flights"] if f["departure_city"] == a["departure_city"] and f["arrival_city"] == a["arrival_city"]]
        keys = ("airline", "flight_number", "departure_time", "arrival_time", "price", "contact_information")
        return text("\n".join(show(f, keys) for f in found) or "no flights found")
    if name == "reserve_hotel":
        reserve("hotel", a["hotel"], a["start_day"], a["end_day"])
        return text(f"Reservation for {a['hotel']} from {a['start_day']} to {a['end_day']} has been made successfully.")
    if name == "reserve_restaurant":
        start = a["start_time"]
        end = f"{start[:11]}{int(start[11:13]) + 2:02d}{start[13:]}"
        reserve("restaurant", a["restaurant"], start, end)
        return text(f"Reservation for {a['restaurant']} from {start} to {end} has been made successfully.")
    if name == "reserve_car_rental":
        reserve("car", a["company"], a["start_time"], a["end_time"])
        return text(f"Reservation for a car at {a['company']} from {a['start_time']} to {a['end_time']} has been made successfully.")
    if name == "search_calendar_events":
        q = a["query"].lower()
        found = [e for e in s["events"] if (q in e["title"].lower() or q in e["description"].lower())
                 and (a.get("date") is None or e["start"].startswith(a["date"]))]
        return text("\n".join(show_event(e) for e in found) or "no events found")
    if name == "get_day_calendar_events":
        return text("\n".join(show_event(e) for e in s["events"] if e["start"].startswith(a["day"])) or "no events")
    if name == "create_calendar_event":
        e = {"id": next_id(s["events"]), "title": a["title"], "description": a.get("description") or "",
             "start": a["start_time"], "end": a["end_time"], "location": a.get("location"),
             "participants": [ACCOUNT, *[p for p in a.get("participants") or [] if p != ACCOUNT]],
             "status": "confirmed"}
        s["events"].append(e)
        return text(show_event(e))
    if name == "cancel_calendar_event":
        e = next((x for x in s["events"] if x["id"] == a["event_id"]), None)
        if e is None:
            return text(f"no event {a['event_id']}", error=True)
        e["status"] = "canceled"
        return text(show_event(e))
    if name == "send_email":
        e = {"id": next_id(s["emails"]), "sender": ACCOUNT, "recipients": a["recipients"],
             "cc": a.get("cc") or [], "bcc": a.get("bcc") or [], "subject": a["subject"], "body": a["body"],
             "status": "sent", "timestamp": f"{TODAY} 12:00"}
        s["emails"].append(e)
        return text(f"email {e['id']} sent")
    return None


def save():
    path = os.environ.get("TRAVEL_STATE")
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
                "serverInfo": {"name": "agentdojo-travel", "version": "1"},
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
