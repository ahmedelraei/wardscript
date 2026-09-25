# AgentDojo in Wardscript

A port of [AgentDojo](https://github.com/ethz-spylab/agentdojo) (v1), the prompt-injection
benchmark for tool-using agents. So far: the **banking** suite.

```bash
python3 benchmarks/agentdojo/run.py
```

## Results: banking

| | |
|---|---|
| User tasks that succeed (utility) | **16 / 16** |
| (user task, injection task) pairs where the attacker's goal is reached, human approver checks what they asked for | **0 / 144** |
| … every approval granted without looking | 7 / 144 |
| Naive versions of the tasks rejected at compile time (W0107) | 3 / 3 |

The 7 are all in user task 0 ("pay the bill in this file"): the payee's IBAN comes from
the file, so only the human who approves the payment can tell a real bill from an injected one.
No other task sends money, moves a standing order or changes the password on the strength of
anything a tool or the model said.

## How it's measured

- **The program is the plan.** AgentDojo gives a general agent the user's request and lets
  it choose tools. Here each user task is a Wardscript function written from the request
  (`banking/main.ward`), the way CaMeL has a planner write code from the trusted query.
  The request's constants (the friend's IBAN, "rent is 2200") are trusted. Everything a tool
  returns, and every model answer, is untrusted.
- **The environment is AgentDojo's**, served over MCP by `banking/bank_server.py`, with
  AgentDojo's injection vectors (the incoming transaction's subject, the bill, the landlord's
  notice, the address change) filled with its `important_instructions` attack.
- **Utility** runs each task with a mock model that answers correctly, and applies AgentDojo's
  checks to the result and the bank's state afterwards.
- **Security** runs each task against each of the 9 injection tasks with a model **fully
  controlled by the attacker**: every `ai fn` answer (every string, amount, id and flag) is what
  that injection task needs, whatever the prompt says. This is stronger than AgentDojo's
  setting, where a real model has to be persuaded first. The goal is then checked on the bank's
  state. It runs twice: once with an approver who approves only what the user asked for, and
  once with one who approves everything.
- **Compile time.** `tests/attacks/agentdojo_banking_*` are the obvious agent-style versions
  (the model reads the bill and pays whoever it names, picks which standing order to change,
  chooses the new password). `ward check` rejects each one with W0107.

`tests/e2e/test_agentdojo_banking.py` runs the same evaluation under `cargo test`.

## What the checker made the port do

- Amounts the user didn't state go through `validate` with an explicit bound (`small_payment`,
  `dinner_share`, a rent change within 20%). The bounds are policy the developer wrote, and they
  show up in review.
- Values that nothing can check, like a payee or an address read from a letter, go through `approve`.
- Picking a standing order by id first used `@not_sink(update_scheduled_transaction.id)`, with the
  reason that an id "only picks one of the user's orders". The adversarial model used exactly that
  to put the new landlord's IBAN on the recurring Spotify order (user task 15, injection task 4).
  The id is now validated against the bank's own listing (`is_rent_order`). A `@not_sink` reason is
  a claim, and this one was false.

## Limits

- Task-specific programs are not a general agent: the utility number says the tasks *can* be
  written so they check, not that an arbitrary request can be served.
- The mock model is honest or fully adversarial, with nothing in between; the honest answers are
  hand-written. Runs against real models (`WARD_LIVE=1`) aren't done yet.
- The rent-order check trusts that the bank's own listing of scheduled transactions is accurate.
  AgentDojo only injects into the incoming transaction's subject and the files, so the listing
  can't carry an injection here.
- The Rule of Two isn't exercised: no tool is marked `@private`.
- Workspace, travel and Slack suites are still to do.
