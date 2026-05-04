---
name: pr-description
description: Generate a pull request title and description from the session's work
---

Based on the work done in this session, write a pull request title and description.

Format:
```
Title: <short, under 70 characters>

## Summary
<1-3 bullet points of what changed and why>

## Changes
<list of files changed with one-line descriptions>

## Testing
<how this was tested or what needs manual verification>
```

Be concise. Focus on the "why", not the "what" — the diff shows the what.
