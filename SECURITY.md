# Security policy

Harness handles model credentials, workspace content, tool effects and local transcripts. Please do
not put a suspected vulnerability, credential, token, private source excerpt or transcript in a
public issue.

## Report privately

Use this repository's **Security** tab and choose **Report a vulnerability**. Include the affected
release or commit, the boundary involved, reproduction steps, and the impact. Use synthetic
credentials and fixtures wherever possible.

If GitHub does not offer the private reporting form, open a public issue containing no sensitive
details and ask a maintainer to establish a private channel.

## Supported versions

Harness is pre-v1. Security fixes target the current `main` branch and the latest tagged release;
older contract directories remain immutable evidence, not supported runtime branches.

## Public source and secrets

The repository is publicly readable under `LicenseRef-B10x-Proprietary`. Public visibility grants
no open-source licence. Credentials, key files and transcripts must never be committed, even in test
fixtures or history.
