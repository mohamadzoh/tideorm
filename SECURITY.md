# Security Policy

## Supported Versions

Security fixes are provided on a best-effort basis for:

- The current `master` branch
- The latest published release of `tideorm` and `tideorm-macros`

Older releases may not receive fixes. If you report a vulnerability against an older version, you may be asked to verify it against the latest release or `master`.

## Reporting A Vulnerability

If you believe you have found a security vulnerability in TideORM, do not open a public GitHub issue.

Please report it privately to:

- `alzoubi528@gmail.com`

Use a subject line such as `TideORM Security Report` and include as much of the following as you can:

- A description of the issue and the suspected impact
- The affected version, feature flags, and backend in use
- Clear reproduction steps or a proof of concept
- Whether the issue can lead to data exposure, SQL injection, privilege escalation, or remote code execution
- Any suggested mitigation or patch direction, if available

Please redact secrets, credentials, tokens, and personal data from your report.

## Scope

This policy covers vulnerabilities in the TideORM project itself, including:

- Query generation and SQL safety issues
- Tokenization, encryption, and hashing behavior
- Macro-generated code that can introduce unsafe or insecure runtime behavior
- Dependency or configuration choices that directly create a vulnerability in TideORM

Reports that are primarily about local deployment mistakes, unsupported forks, or third-party infrastructure outside this repository may not be treated as project vulnerabilities, though they may still help improve documentation.

## Disclosure Process

- Reports will be reviewed privately.
- Coordinated disclosure is preferred.
- Please avoid public disclosure until a fix or mitigation has been prepared and maintainers have had a reasonable chance to respond.

If the report is confirmed, the fix may be released on `master`, in a published crate release, or both, depending on severity and impact.

## Responsible Research

Good-faith security research is welcome. Please avoid actions that would:

- Access, modify, or destroy data that does not belong to you
- Disrupt services or degrade repository availability
- Expose sensitive information unnecessarily

Keep testing limited, targeted, and reversible.