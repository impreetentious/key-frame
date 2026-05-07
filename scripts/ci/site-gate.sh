#!/usr/bin/env bash
#
# The projection room, built and driven the way a visitor gets it.
#
# The smoke suite runs against the production build served statically, not
# against the development server: a page that works under a bundler's dev
# pipeline and fails as static files fails for everyone who visits it.
#
# Order matters. The build fails first if the catalogue names a regression
# stream that is not in the repository, then the budget fails if the download
# grew past what the page promises, then the browser fails if the page does not
# actually decode, scrub, overlay, and answer a click.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

"$repo_root/scripts/build-inspector.sh"
node "$repo_root/scripts/ci/site-budget.mjs"
npm --prefix inspector run smoke

echo "site-gate: OK — the site builds, fits its budget, and decodes in a real browser"
