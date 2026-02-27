# UX & Panel Audit Findings

## Overview
The Admin Panel Settings page (`settings.html`) and its handler (`settings.rs`) have accumulated significant technical debt and UI redundancy. The "System" tab is particularly overloaded with advanced configuration that should likely be separated or simplified.

## UI Redundancies & Issues

### 1. Duplicated Save Logic
- **Issue:** The "Trials" tab has its own `<form hx-post="...">` and submit button, while the rest of the page uses a hidden global form (`#main-settings-form`) triggered by a sticky footer button.
- **Confusion:** Users might click the global "Save All Changes" button expecting trial settings to update, but they won't if they haven't submitted the specific trial form.
- **Fix:** Unify all settings into a single form structure or clearly separate distinct "Tools" pages.

### 2. Overloaded "System" Tab
- **Issue:** The "System" tab contains:
    - Deployment Topology (Hub vs Distributed toggles).
    - Update Center (Hub update).
    - Agent Rollout (Node update).
    - Worker Rollout (Sub/Bot update).
    - Database Backup.
    - Decoy Traffic.
    - Kill Switch.
- **Clutter:** This mix of "set-and-forget" infrastructure config and "active operations" (rollouts, backups) makes the tab very long and intimidating.
- **Fix:** Split "Updates & Rollouts" into a dedicated page/tab. Move "Backup" to a distinct "Maintenance" area.

### 3. Confusing Update Flows
- **Issue:** There are 4 different update mechanisms displayed:
    1.  Control-plane local upgrade.
    2.  Agent rollout (global signal).
    3.  Sub/Bot worker manual queueing.
    4.  Manual CLI commands shown as text.
- **Risk:** It's unclear to the user which button updates *what*. The "Prepare from Latest Release" button for agents is obscure.

### 4. Advanced Controls Hidden in `<details>`
- **Issue:** Critical features like "Agent Rollout" and "Worker Rollout" are hidden inside `<details>` tags under "Advanced Controls".
- **Risk:** These are core operational tasks in a distributed system, not "optional advanced settings". They should be first-class citizens.

### 5. Bot Settings Split
- **Issue:** Bot Token and Username are in "General", but "Bot Interface Mode" and "Support Button" are also there. Meanwhile, "Local Bot" toggle is in "System".
- **Fix:** consolidate all Bot-related settings into a "Bot" tab or page.

## proposed_roadmap.md Preview

1.  **Refactor Settings Page:**
    - Split into: `General`, `Billing/Plans`, `Bot`, `Security (Decoy/KillSwitch)`, `System (Updates/Topology)`.
    - Unify form submission logic.
2.  **Dedicated Update Center:**
    - Create a new top-level page or prominent tab for "Version Management".
    - Show a unified matrix of [Component | Current Version | Target Version | Status].
3.  **Simplify Topology UI:**
    - Instead of manual "Local Sub/Bot" toggles, auto-detect based on running services and provide a high-level "Mode" switch (Hub vs Distributed) that presets these.
