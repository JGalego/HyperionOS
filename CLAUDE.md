# CLAUDE.md

# Hyperion Development Guide

> **Hyperion** is the first intent-native operating system.
>
> Humans express goals.
> Hyperion determines how those goals become reality.

---

# Mission

Hyperion is **not** another operating system with an AI assistant.

The AI **is** the operating system.

Every subsystem should be designed around one question:

> **How can this make accomplishing a human goal easier?**

If a proposed implementation merely recreates existing operating system behavior with an LLM attached, reject it and redesign it.

---

# Project Philosophy

Traditional operating systems expose implementation details.

Users think about:

- applications
- files
- folders
- windows
- drivers
- package managers
- terminals
- settings

Hyperion should expose none of these unless explicitly requested.

Users think only about their goals.

Example:

Instead of:

- Open Photoshop
- Open Finder
- Export PNG
- Save As...

The user simply says:

> "Create a social media graphic from these photos."

Hyperion chooses the implementation.

---

# Guiding Principles

## Human-first

Every decision should optimize for human understanding before technical elegance.

Never expose complexity unnecessarily.

---

## Intent over Implementation

Never ask:

"What application should we use?"

Instead ask:

"What capability is required?"

---

## Local First

Assume computation should happen locally.

Only use cloud resources when there is a measurable benefit.

Users should own their data.

---

## Explainability

Every autonomous action should be explainable.

Hyperion must always be able to answer:

- Why?
- How?
- What evidence?
- Confidence?
- Undo?

---

## User Control

Hyperion assists.

It does not control.

Every autonomous action must be:

- interruptible
- reversible
- inspectable
- configurable

---

## Accessibility First

Accessibility is architecture.

Not a feature.

Every interface must work for:

- keyboard
- mouse
- touch
- voice
- screen readers
- assistive technologies

Never build inaccessible features.

---

## Progressive Complexity

Never overwhelm users.

Beginners should feel comfortable.

Experts should never feel constrained.

Complexity should appear naturally.

---

# Engineering Principles

## Build for longevity

Avoid trendy technologies simply because they are popular.

Favor:

- maintainability
- modularity
- observability
- testability
- reliability

---

## Design APIs before implementation

Before writing significant code:

Define:

- interfaces
- contracts
- responsibilities

Then implement.

---

## Small composable systems

Avoid giant classes.

Avoid giant services.

Prefer:

- independent modules
- well-defined interfaces
- dependency injection
- message passing

---

## Capability-based architecture

Hyperion is not application-centric.

Everything should be implemented as reusable capabilities.

Examples:

Instead of:

ImageEditor

Create:

Image Editing Capability

Instead of:

Spreadsheet App

Create:

Tabular Data Capability

---

## Intent-first architecture

Internal execution should look like:

User Goal

↓

Intent

↓

Planner

↓

Capability Selection

↓

Execution Plan

↓

Agents

↓

Kernel Services

↓

Hardware

Never the reverse.

---

# Coding Standards

## General

Write clean, modern, maintainable code.

Avoid clever code.

Readable code always wins.

---

## Functions

Functions should:

- do one thing
- have descriptive names
- be deterministic whenever possible
- avoid hidden state

---

## Files

Keep files focused.

Split large files before they become difficult to navigate.

---

## Comments

Explain:

WHY

not

WHAT

Good:

// We cache semantic embeddings to reduce repeated inference.

Bad:

// Increment i

---

## Naming

Use descriptive names.

Good:

IntentPlanner

SemanticObjectStore

WorkspaceGenerator

ContextResolver

Bad:

Manager

Util

Helper

Misc

Common

---

# Architecture Rules

Maintain strict separation between:

Kernel

↓

Runtime

↓

Intent Engine

↓

Planning Engine

↓

Agent Runtime

↓

Capabilities

↓

User Interface

No layer should violate architectural boundaries.

---

# Memory

Hyperion has multiple memory systems.

Do not merge them into one database.

Examples:

Working Memory

Semantic Memory

Procedural Memory

Episodic Memory

User Preferences

Each should have clear responsibilities.

---

# AI Design

Never assume a single LLM.

Hyperion should support multiple models.

Possible model roles:

Planning

Coding

Vision

Speech

Translation

Reasoning

Fast chat

Long-context

Models should be replaceable.

Never tightly couple implementation to a specific vendor.

---

# Context

Context is one of Hyperion's defining features.

Whenever implementing a feature ask:

"What context should already exist?"

Avoid forcing users to repeat themselves.

---

# User Experience Rules

Every interface should satisfy these questions:

Can a non-technical user understand this?

Would a grandparent understand this?

Can a child use this?

Can an expert work efficiently?

Can this action be undone?

Does this reduce cognitive load?

If the answer is "no", redesign.

---

# Errors

Never expose technical errors directly.

Bad:

NullPointerException

Good:

"I couldn't finish importing your photos because one appears to be corrupted."

Include technical logs separately for developers.

---

# Security

Prefer capability-based security.

Avoid global permissions whenever possible.

Every action should be:

minimal

auditable

explainable

reversible

---

# Performance

Optimize for perceived responsiveness.

Users should never wait unnecessarily.

Prefer:

streaming

incremental rendering

background work

predictive loading

Avoid blocking operations.

---

# Testing

Every new feature should include:

Unit tests

Integration tests

Regression tests

Failure cases

Edge cases

Accessibility validation

Performance validation

---

# Documentation

Every public component should document:

Purpose

Inputs

Outputs

Failure modes

Performance considerations

Security implications

---

# Pull Request Expectations

Every pull request should answer:

## What problem does this solve?

## Why is this solution correct?

## What alternatives were considered?

## Does this increase or reduce complexity?

## How was it tested?

## Does it improve the user experience?

---

# When Designing New Features

Always begin by asking:

1. What human goal is being solved?

2. Should this be automatic?

3. Can this require fewer clicks?

4. Can the user avoid learning something technical?

5. Can Hyperion infer the intent safely?

6. Is there a simpler solution?

Only then begin implementation.

---

# What Claude Should Optimize For

When contributing to Hyperion, prioritize in this order:

1. Simplicity
2. User experience
3. Accessibility
4. Correctness
5. Reliability
6. Security
7. Performance
8. Maintainability
9. Extensibility
10. Elegance

Never sacrifice simplicity for cleverness.

---

# Things Claude Should Challenge

If any proposed feature:

- adds unnecessary complexity
- requires users to think technically
- duplicates existing functionality
- introduces avoidable configuration
- tightly couples components
- makes future maintenance harder
- leaks implementation details to users

then propose a better design before writing code.

Do not blindly implement poor architecture.

---

# Definition of Done

A feature is only complete if it is:

- Correct
- Tested
- Accessible
- Documented
- Observable
- Explainable
- Secure
- Undoable where appropriate
- Maintainable
- Consistent with Hyperion's philosophy

Working code alone is **not** considered finished.

---

# North Star

Every contribution should move Hyperion closer to this vision:

> **A computer that understands people, instead of requiring people to understand computers.**