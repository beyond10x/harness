---
format: aep.planning-md/1
id: review-result:harness-public-site-sweep
kind: review-result
status: active
title: Harness public website and documentation sweep
summary: Publication, governance, factual, reference, information-architecture, accessibility, and repository-security findings at 0.6.0.
tags:
- docs
- review
- security
relations:
- reviews: vision:b10x-owns-its-loop
revision: 1
---
## Scope

Reviewed the public Docusaurus projection, landing page, navigation, deployment workflow, generated CLI contract, repository visibility/settings, and the public claims against Harness 0.6.0 at `4fdcee0`.

## Findings

1. **Critical — the advertised public site does not exist.** `https://beyond10x.github.io/harness/` returns 404. Every Pages run fails in `actions/configure-pages` because Pages was never enabled.
2. **High — public visibility is ungoverned and contradicts both repositories.** GitHub reports Harness public while Harness says private and Atlas maps it private. Atlas has no decision authorising this exposure.
3. **High — credential safety prose contradicts shipped behaviour.** The quickstart and security page deny vendor-directory defaults and renewal while built-in providers declare credential paths and `codex` may renew and rewrite its default store before a run.
4. **Medium — the public status understates live evidence.** Both subscription routes have authorised live observations, while the pinned request bytes remain provider-emulated; the site collapses those distinct claims.
5. **Medium — the CLI reference is incomplete and can drift silently.** `--driver` and `--instructions-file` are absent, and no gate compares the public reference with clap's generated contract.
6. **Medium — task guidance and exact reference are interleaved.** The provider and workflow guides are long design narratives, link internal engineering records, and make first-use tasks hard to follow.
7. **Medium — the landing page makes a false credential metric and has accessibility defects.** The synthetic wire selector uses incomplete tab semantics, JavaScript smooth scrolling ignores reduced motion, small text has weak contrast, and narrow layouts remain cramped.
8. **Medium — public-repository reporting and prevention controls are absent.** Private vulnerability reporting, secret scanning, push protection, Dependabot security updates, a repository homepage, and a root reporting policy are unset.

## Secret-scan observation

Checksum-pinned Gitleaks 8.30.1 scanned 144 reachable commits and 6.33 MB. Four candidates were classified without exposing their values: one Cargo dependency-name line, two synthetic contract cache keys, and one test environment-variable name. Exact fingerprint suppressions and a green final rescan are required; a broad path or commit suppression is not acceptable.

## Required closure

Every finding is tracked by a Protocol story. Atlas records the public-visibility decision while remaining private. Existing public routes remain valid, the current product stays 0.6.0, Rust checks prevent CLI/version/internal-link drift, the full Harness and Atlas gates pass, public-repository controls are enabled, and the deployed Pages artifact for the merged commit returns HTTP 200.
