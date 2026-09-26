# Plan: rules for using AI in contributions

**Status:** Plan, not yet adopted.

Wardscript has rules for coding agents (`AGENTS.md`) but no rules for the *people*
who use them, and no `CONTRIBUTING.md` at all. This plan adds a short AI policy,
modeled on CPython's, and adapts it to what can actually go wrong here.

## 1. The live case: CPython

CPython's devguide has one page, "Guidelines for using AI tools"
(`getting-started/ai-tools.rst` in `python/devguide`, linked from the devguide's
index). It is short and has four parts:

| Part | What it says |
|---|---|
| Responsibility | "The person submitting an issue or PR is responsible for its content, regardless of whether AI tools were used." |
| Considerations for success | Review AI output in detail before submitting. Be able to explain the change "in their own words". Disclosure is "appreciated, while not required". General quality rules apply to everyone: is the change necessary, minimal and focused, follows existing style, has tests, keeps backwards compatibility. Review titles and descriptions too. |
| Acceptable uses | Writing comments (especially in a non-native language), understanding existing code, supplementing knowledge for code, tests and docs. |
| Unacceptable uses | Maintainers may close unproductive issues and PRs without explanation, AI or not. Repeat offenders may be blocked. Never alter or bypass tests, or remove functionality, to make a failing test pass. |

What to take from it:

- **It regulates the contribution, not the tool.** Nothing is banned outright; the
  author owns the result. This is enforceable, since nobody can reliably detect
  AI-written code.
- **It's short.** One page that people actually read.
- **The one hard rule is about tests.** That is the characteristic failure of
  AI agents: making the red test green by weakening it.
- **Low-effort closes need no justification**, which protects maintainer time.

What we'd do differently:

- **Disclosure: require it, lightly.** CPython only appreciates it. Wardscript is a
  language whose whole pitch is "know where your data came from"; asking for the
  provenance of a PR is on-brand and cheap (one checkbox). We still don't ban use.
- **Name our own test-bypass cases.** CPython's rule is generic. Ours has concrete
  equivalents: `cargo insta accept`, `tests/attacks/` and diagnostic codes.
- **Point agents at the rules.** CPython's page is for humans. We already have
  `AGENTS.md`, so agents can be told the same rules directly.

## 2. What goes wrong here specifically

| Risk | Why it matters in Wardscript |
|---|---|
| Blindly accepted snapshots | `cargo insta accept` turns any diagnostic regression into the new "expected" output. |
| Weakened attack tests | `tests/attacks/` programs **must fail to compile**. Editing one so it "passes", or deleting it, removes a security guarantee silently. |
| Checker changes nobody understands | The checker is the product. A plausible-looking change to label propagation or implicit flow can open a path from `Untrusted` to a sink. |
| Invented WEPs | A WEP records *why*. A generated one argues well but may not reflect a real design decision, or may skip the Trust and security section's actual reasoning. |
| Changed diagnostic meanings | Codes (`W0xxx`) are stable by rule; agents renumber or reword them freely. |
| Fabricated benchmark numbers | The README's AgentDojo table is a claim. Numbers must come from a run, not from a model. |
| Live-model tests and secrets | `WARD_LIVE=1` needs API keys; agents may paste keys or recorded responses containing them. |
| Issue and PR spam | Generated "bug reports" against a small project with one maintainer. |

## 3. Proposed deliverables

### 3.1 `CONTRIBUTING.md` (new)

A general contributing guide, since none exists: build and test commands (from
`AGENTS.md`), when a WEP is needed (link `docs/weps/000-process.md`), licensing
(move the note from the README), and a short **Using AI tools** section that links
to the full policy. Keeping the AI rules in their own file makes them easy to link
from PRs.

### 3.2 `docs/ai-policy.md` (new): the policy

Same shape as CPython's page, about one screen long:

1. **You are responsible.** The submitter owns every line, AI-assisted or not, and
   must be able to explain it in their own words and answer review questions
   without relaying them to a model.
2. **Disclose.** Tick the PR template box and say briefly what the tool did
   (e.g. "wrote the first draft of the parser tests"). `Co-Authored-By` trailers
   from agents are welcome and count as disclosure.
3. **Acceptable uses.** Understanding the codebase, drafting code and tests you
   then review, writing or polishing English, finding bugs, reviewing your own diff.
4. **Hard rules (a PR breaking these is closed):**
   - Don't weaken, delete or skip a test to make it pass. That includes accepting
     snapshots you haven't read: every `.snap` change must be explained in the PR.
   - Don't change a `tests/attacks/` case so that it compiles, and don't remove one.
     New trust behavior needs a new attack case, not an edited one.
   - Don't change the meaning of an existing diagnostic code.
   - Don't submit benchmark, performance or AgentDojo numbers you didn't produce by
     running the code; say how they were produced.
   - Don't include API keys, tokens, or unredacted recorded responses.
5. **WEPs are written by people.** AI can help with wording, but the motivation,
   the rejected alternatives and the Trust and security argument must be the
   author's own reasoning.
6. **Issues.** Reproduce before filing: include a `.ward` snippet and the actual
   `ward check` output. Don't file unverified generated reports.
7. **Maintainer discretion.** Unproductive issues and PRs may be closed without
   explanation, AI or not. Repeated ones may lead to a block. (Taken from CPython
   nearly verbatim.)

### 3.3 `.github/pull_request_template.md` (new)

Short, with a checklist:

- [ ] I can explain every change in this PR myself.
- [ ] AI tools were used: *(none / what and how)*
- [ ] Any snapshot (`.snap`) changes are listed and explained below.
- [ ] No test in `tests/attacks/` was edited or removed. *(or: explain why)*
- [ ] Needs a WEP? *(no / link)*

### 3.4 Issue templates (new, optional)

`.github/ISSUE_TEMPLATE/bug.yml` with required fields: `.ward` snippet, command
run, actual output, expected output. This makes unreproduced generated reports
easy to spot and close.

### 3.5 `AGENTS.md` (update)

Add a **Contribution rules** section so agents follow the policy without being
told: never run `cargo insta accept` without reporting each changed snapshot;
never edit or delete files in `tests/attacks/` to make them compile; don't invent
benchmark numbers; mention AI use in the PR description. Link `docs/ai-policy.md`.

### 3.6 CI guard (optional, cheap)

A job in `.github/workflows/ci.yml` that fails when a PR modifies or deletes
anything under `tests/attacks/` unless the PR has a `attack-test-change` label
applied by the maintainer. Additions stay allowed. This is the one rule that can
be enforced mechanically, and it protects the core guarantee regardless of who or
what wrote the change.

### 3.7 Website (`wardscript-website`, follow-up)

The docs sync copies only `docs/weps/` and `docs/img/`. Link the policy from the
site's docs index (a "Contributing" link to the GitHub file) rather than syncing
it, so there's one copy.

## 4. Rollout

1. Open a PR with 3.1, 3.2, 3.3 and 3.5. No WEP is needed: the WEP process covers
   the language, not contribution rules.
2. Maintainer review of the policy wording, in particular the disclosure rule
   (required vs. appreciated) and whether rule 5 (WEPs) is too strict.
3. Second PR: CI guard (3.6) and issue template (3.4).
4. Website link (3.7).
5. Revisit after three months or ~20 external PRs: which rules were hit, whether
   disclosure is being given, whether the attack-test guard caused friction.

## 5. Open questions for the maintainer

- Required or appreciated disclosure? (Plan recommends required, as a checkbox.)
- Should AI-written WEPs be allowed if disclosed, or ruled out?
- Is the CI label gate for `tests/attacks/` worth the friction for a
  single-maintainer project?
