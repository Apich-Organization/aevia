# Platform operations

The production layout comprises a public reverse proxy, a private registry application and PostgreSQL, and durable archive storage. The optional book runner is hosted separately under a dedicated rootless Podman account. Bind databases and private execution endpoints only to their required network interfaces.

## Build and configure

Build the registry image from the platform context and the sibling language-crate context. The reduced workspace manifest used in the image is checked for drift against the actual compiler workspace. The application uses a database URL, a secret used to verify sessions, a public base URL, and explicit paths for archive data and document mounts. See the [deployment runbook](https://github.com/Apich-Organization/aevia/blob/v0.1/platform/docs/DEPLOY.md) for environment variables, Compose, Quadlet and backup commands.

## Roll out and check

After deployment, validate the published origin over HTTPS, the health endpoint and a registry read. Verify document pages for both languages and test an offline HTML download. If the playground is enabled, run a short sample and observe success and error handling. Confirm the registry's environment and mounts exclude the private signing and container control interfaces used by administrators and other services.

Use a pinned image and preserve PostgreSQL and archive backups. An upgrade follows its migration compatibility rules. Check restores against a separate database and a separate archive destination. Select a database and archive snapshot from the same release state, then verify the index against available archives before directing traffic to a restored service.

## Update content

The mount directories provide canonical Markdown sources, or a generated JSON reference. A poller detects changes and loads the candidate snapshot before publishing it. Stage complete directory or file replacements where possible. Keep counterpart chapter slugs the same in both languages. A failed reload leaves the last valid snapshot active and records a diagnostic for the operator.
