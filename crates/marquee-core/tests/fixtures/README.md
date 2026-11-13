# API fixtures

These are the canned responses the API-client tests run against, so CI never makes a live call.

They were written by hand from each service's published response shapes (trimmed to the fields
Marquee reads). They were **not** recorded from live calls, because the build environment had no
API keys. If a live response ever differs from one of these, replace the file with the real
response (redact tokens) and the tests will tell you what to fix.
