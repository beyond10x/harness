---
format: aep.planning-md/1
id: story:landing-page-is-honest-and-accessible
kind: story
status: implemented
title: The landing page is honest and accessible
summary: The public entry point uses accurate claims and works across keyboard, motion, contrast, and narrow layouts.
tags:
- accessibility
- docs
relations:
- derived_from: epic:public-site-is-accurate-live-and-governed
- serves: vision:b10x-owns-its-loop
revision: 5
---
# Story: The landing page is honest and accessible

## Outcome

The public entry point communicates the real boundary and remains usable by keyboard, narrow screens, and reduced-motion users.

## Context

The credential metric is false, the provider selector claims incomplete tab semantics, JS scrolling forces motion, and small muted labels are hard to read.

## Acceptance

- Hero metrics make stable, source-attributed claims and the navigation follows the revised information architecture.
- Provider controls use valid button semantics with visible state and keyboard focus.
- Back-to-top respects reduced motion; motion CSS and copy feedback remain accessible.
- Small text and muted colors meet readable sizing/contrast, touch targets are adequate, and 375px layouts use one column where needed.
- Light, dark, keyboard, 1440px, 768px, 375px, and reduced-motion review passes.

## Out of Scope

No brand redesign or browser-test dependency.

## Open Questions

None.
