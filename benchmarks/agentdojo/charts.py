"""Draws the README's AgentDojo charts as SVG, in light and dark (the SVGs follow the
viewer's color scheme). The numbers are `run.py`'s and `run.py --program blind`'s;
update them here when those change.

    python3 benchmarks/agentdojo/charts.py
"""

import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..")
OUT = os.path.join(ROOT, "docs", "img")

PAIRS = 489
# (row, got through past a careful approver, reached only if every approval is granted)
ATTACKS = [("Our port", 0, 8), ("Blind port", 4, 54)]
# suite -> (tasks, ours, blind)
UTILITY = [("Banking", 16, 16, 16), ("Slack", 21, 21, 16), ("Workspace", 40, 40, 39)]

STYLE = """
<style>
  .surface { fill: #fcfcfb; }
  .t1 { fill: #0b0b0b; } .t2 { fill: #52514e; } .grid { stroke: #e4e3df; }
  .s1 { fill: #2a78d6; } .s2 { fill: #eb6834; }
  .good { fill: #0ca30c; } .warn { fill: #fab219; } .crit { fill: #d03b3b; }
  .gap { stroke: #fcfcfb; }
  text { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif; }
  @media (prefers-color-scheme: dark) {
    .surface { fill: #1a1a19; }
    .t1 { fill: #ffffff; } .t2 { fill: #c3c2b7; } .grid { stroke: #383835; }
    .s1 { fill: #3987e5; } .s2 { fill: #d95926; }
    .gap { stroke: #1a1a19; }
  }
</style>"""


def svg(width, height, title, desc, body):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
            f'viewBox="0 0 {width} {height}" role="img" aria-labelledby="t d">'
            f'<title id="t">{title}</title><desc id="d">{desc}</desc>{STYLE}'
            f'<rect class="surface" width="{width}" height="{height}" rx="8"/>{body}</svg>\n')


def text(x, y, s, cls="t1", size=13, anchor="start", weight="normal"):
    return (f'<text x="{x}" y="{y}" class="{cls}" font-size="{size}" text-anchor="{anchor}" '
            f'font-weight="{weight}">{s}</text>')


def swatch(x, y, cls, label):
    return f'<rect x="{x}" y="{y - 10}" width="12" height="12" rx="3" class="{cls}"/>' + text(x + 18, y, label, "t2", 12)


def bar_end(x, y, w, h, cls, round_right):
    """A bar whose data end (right) is rounded 4px and whose baseline end is square."""
    if w <= 0:
        return ""
    r = min(4, w / 2, h / 2) if round_right else 0
    return (f'<path class="{cls}" d="M{x},{y} h{w - r} a{r},{r} 0 0 1 {r},{r} v{h - 2 * r} '
            f'a{r},{r} 0 0 1 -{r},{r} h-{w - r} z"/>')


def attacks():
    width, left, bar_w, bar_h = 760, 120, 600, 22
    body = text(24, 32, f"AgentDojo: what happened to {PAIRS} attacks", size=15, weight="600")
    body += text(24, 52, "User task × injection task pairs, banking + Slack + workspace; the attacker controls every model answer", "t2", 12)
    legend = [("good", "Blocked by the program"), ("warn", "Stopped only by a human approving"), ("crit", "Got through")]
    x = 24
    for cls, label in legend:
        body += swatch(x, 80, cls, label)
        x += 30 + len(label) * 6.6
    y = 104
    for name, through, approver in ATTACKS:
        blocked = PAIRS - through - approver
        body += text(24, y + 16, name, weight="600")
        cx = left
        segments = [(blocked, "good"), (approver, "warn"), (through, "crit")]
        last = max(i for i, (n, _) in enumerate(segments) if n)
        for i, (n, cls) in enumerate(segments):
            w = bar_w * n / PAIRS
            if n:
                # Tiny segments still get a visible sliver; the labels below carry the number.
                w = max(w, 3)
                body += bar_end(cx, y, w, bar_h, cls, i == last)
                if i < last:
                    body += f'<line class="gap" x1="{cx + w}" y1="{y}" x2="{cx + w}" y2="{y + bar_h}" stroke-width="2"/>'
                cx += w
        label = f"{blocked} blocked · {approver} needed a human · "
        body += text(left, y + bar_h + 18, label + f'<tspan class="t1" font-weight="600">{through} got through</tspan>', "t2", 12)
        y += 70
    desc = "; ".join(f"{n}: {PAIRS - t - a} blocked, {a} stopped only by a human approver, {t} got through"
                     for n, t, a in ATTACKS)
    return svg(width, y - 4, f"AgentDojo: what happened to {PAIRS} attacks", desc, body)


def utility():
    width, left, bar_w, bar_h = 760, 120, 520, 16
    body = text(24, 32, "AgentDojo: user tasks that succeed", size=15, weight="600")
    body += text(24, 52, "Share of each suite's tasks, with scripted (honest) model answers", "t2", 12)
    body += swatch(24, 80, "s1", "Our port") + swatch(120, 80, "s2", "Blind port, written without seeing the attacks")
    y = 104
    for x in (0, 50, 100):
        gx = left + bar_w * x / 100
        body += f'<line class="grid" x1="{gx}" y1="{y - 6}" x2="{gx}" y2="{y + 3 * 58 - 14}" stroke-width="1"/>'
        body += text(gx, y + 3 * 58 + 2, f"{x}%", "t2", 11, "middle")
    for suite, tasks, ours, blind in UTILITY:
        body += text(24, y + 21, suite, weight="600")
        for i, (n, cls) in enumerate(((ours, "s1"), (blind, "s2"))):
            by = y + i * (bar_h + 2)
            w = bar_w * n / tasks
            body += bar_end(left, by, w, bar_h, cls, True)
            body += text(left + w + 8, by + 12, f"{n}/{tasks}", "t2", 12)
        y += 58
    desc = "; ".join(f"{s}: our port {o}/{t}, blind port {b}/{t}" for s, t, o, b in UTILITY)
    return svg(width, y + 18, "AgentDojo: user tasks that succeed", desc, body)


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, draw in (("agentdojo-attacks.svg", attacks), ("agentdojo-utility.svg", utility)):
        with open(os.path.join(OUT, name), "w", encoding="utf-8") as f:
            f.write(draw())
        print(f"wrote docs/img/{name}")


if __name__ == "__main__":
    main()
