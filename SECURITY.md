# Security Policy

## Supported Versions

Panther is in early development. The project has not published a release
yet. Security fixes apply only to the current state of the `main` branch
until the project publishes its first release.

## Reporting a Vulnerability

Do not open a public issue for a security vulnerability.

Report a security vulnerability through GitHub's private vulnerability
reporting feature:

1. Go to the repository **Security** tab.
2. Select **Report a vulnerability**.
3. Describe the issue, the affected component, and the steps needed to
   reproduce it.

A maintainer will acknowledge the report and follow up with next steps.

## Scope

Treat the following as untrusted input when you evaluate a potential
vulnerability:

- Web content, including HTML, CSS, images, and fonts.
- Network responses, including headers, redirects, and TLS data.
- Local files opened by the browser.
- Data read from local storage, cache, or history.

Memory-safety issues, sandbox or isolation bypasses, and privacy leaks
(such as unintended exposure of cookies, credentials, or private browsing
data) are all in scope.
