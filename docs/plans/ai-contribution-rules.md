# Plan: rules for using AI in contributions

**Status:** Plan, not yet adopted.

Wardscript has rules for coding agents (`AGENTS.md`) but no rules for the *people*
who use them, and no `CONTRIBUTING.md` at all. This plan adds a short AI policy, based on
CPython's "Guidelines for using AI tools" and adapted to what can go wrong here.

## 1. What goes wrong here specifically

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

## 2. Proposed deliverables

### 2.1 `CONTRIBUTING.md` (new)

A general contributing guide, since none exists: build and test commands (from
`AGENTS.md`), when a WEP is needed (link `docs/weps/000-process.md`), licensing
(move the note from the README), and a short **Using AI tools** section that links
to the full policy. Keeping the AI rules in their own file makes them easy to link
from PRs.

### 2.2 `docs/ai-policy.md` (new): the policy

About one screen long:

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
   explanation, AI or not. Repeated ones may lead to a block.

### 2.3 `.github/pull_request_template.md` (new)

Short, with a checklist:

- [ ] I can explain every change in this PR myself.
- [ ] AI tools were used: *(none / what and how)*
- [ ] Any snapshot (`.snap`) changes are listed and explained below.
- [ ] No test in `tests/attacks/` was edited or removed. *(or: explain why)*
- [ ] Needs a WEP? *(no / link)*

### 2.4 Issue templates (new, optional)

`.github/ISSUE_TEMPLATE/bug.yml` with required fields: `.ward` snippet, command
run, actual output, expected output. This makes unreproduced generated reports
easy to spot and close.

### 2.5 `AGENTS.md` (update)

Add a **Contribution rules** section so agents follow the policy without being
told: never run `cargo insta accept` without reporting each changed snapshot;
never edit or delete files in `tests/attacks/` to make them compile; don't invent
benchmark numbers; mention AI use in the PR description. Link `docs/ai-policy.md`.

### 2.6 CI guard (optional, cheap)

A job in `.github/workflows/ci.yml` that fails when a PR modifies or deletes
anything under `tests/attacks/` unless the PR has a `attack-test-change` label
applied by the maintainer. Additions stay allowed. This is the one rule that can
be enforced mechanically, and it protects the core guarantee regardless of who or
what wrote the change.

### 2.7 Website (`wardscript-website`, follow-up)

The docs sync copies only `docs/weps/` and `docs/img/`. Link the policy from the
site's docs index (a "Contributing" link to the GitHub file) rather than syncing
it, so there's one copy.

## 3. Rollout

1. Open a PR with 2.1, 2.2, 2.3 and 2.5. No WEP is needed: the WEP process covers
   the language, not contribution rules.
2. Maintainer review of the policy wording, in particular the disclosure rule
   (required vs. appreciated) and whether rule 5 (WEPs) is too strict.
3. Second PR: CI guard (2.6) and issue template (2.4).
4. Website link (2.7).
5. Revisit after three months or ~20 external PRs: which rules were hit, whether
   disclosure is being given, whether the attack-test guard caused friction.

## 4. Open questions for the maintainer

- Required or appreciated disclosure? (Plan recommends required, as a checkbox.)
- Should AI-written WEPs be allowed if disclosed, or ruled out?
- Is the CI label gate for `tests/attacks/` worth the friction for a
  single-maintainer project?
