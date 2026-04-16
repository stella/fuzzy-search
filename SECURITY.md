# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |

## Reporting a Vulnerability

If you discover a security vulnerability in this
package, please report it responsibly.

**Do not open a public GitHub issue.**

Instead, email **security@stll.app** with:

1. A description of the vulnerability.
2. Steps to reproduce.
3. The affected version(s).
4. Any potential impact assessment.

We will acknowledge your report within 48 hours and
aim to provide a fix or mitigation within 7 days
for critical issues.

## Scope

This package is a native addon (NAPI-RS) implementing
Myers' bit-parallel fuzzy matching algorithm in Rust.
Security concerns may include:

- Memory safety issues in the Rust/NAPI boundary.
- Denial of service via crafted input patterns or
  haystacks (e.g., quadratic blowup in match
  extraction).
- Incorrect boundary handling leading to out-of-
  bounds reads.
- Information leakage through match offsets on
  untrusted input.
