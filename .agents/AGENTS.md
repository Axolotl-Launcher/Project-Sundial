<!-- SPDX-FileCopyrightText: 2026 Axolotl Team -->
<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Agent documentation

## Maintained documentation

`docs/` holds maintained, topic-specific documentation for AI agents, such as
crate boundaries and tool workflows. Keep repository-wide instructions that
agents need at the start of a task in the [root guide](../AGENTS.md).

Before creating a document in `docs/`, ask the user for explicit approval.
State the proposed topic, path and reason it needs a dedicated document. An
explicit user request to create that document supplies the approval.

Keep working drafts, scratch notes, task logs and intermediate output outside
`docs/`. Update an existing guide when it already covers the topic.

## Skills

`skills/` contains reusable instructions for specific kinds of work in this
repository, such as Cargo workflows and GPUI Kit development. Each skill has
its own directory with a `SKILL.md` that describes when to use it and how to
carry out the work.

When a task matches a skill, read its `SKILL.md` before making related changes.
Resolve any relative references from that skill's directory.

## Temporary plans

Use `plans/` for temporary plans, working drafts, scratch notes and other
files that agents need while completing a task. Git ignores this directory.
Create it locally if needed.

Keep all repository code independent of `plans/`. Application code, scripts,
tests, build tools and CI must never read from, write to, reference or depend
on the directory or its contents.
