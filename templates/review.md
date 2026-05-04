---
name: review
description: Review the last Claude run's changes for correctness and issues
---

Review the changes from the last Claude run. Check for:
- Correctness — does the code do what was asked?
- Bugs — off-by-one errors, null handling, race conditions
- Security — injection, XSS, leaked secrets, unsafe input handling
- Missing error handling or edge cases
- Whether the changes match what the team requested

Provide specific feedback with file paths and line numbers. If everything looks good, say so briefly.
