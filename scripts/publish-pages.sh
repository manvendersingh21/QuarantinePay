#!/usr/bin/env bash
# Publish the GitHub Pages showcase (site/index.html + demo video + evidence) to the gh-pages
# branch, pointing its "Open live demo" button at the given public URL.
# Usage: scripts/publish-pages.sh https://<current>.trycloudflare.com
set -euo pipefail
live="${1:?public base URL required}"
case "$live" in https://*) ;; *) echo "public URL must be https"; exit 2 ;; esac
live="${live%/}"
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
work="$root/.local/gh-pages"
cd "$root"
if [ ! -d "$work/.git" ] && [ ! -f "$work/.git" ]; then
  if git ls-remote --exit-code --heads origin gh-pages >/dev/null 2>&1; then
    git fetch -q origin gh-pages
    git worktree add -q "$work" gh-pages
  else
    # Requires git >= 2.42: a new, empty orphan branch in its own worktree.
    git worktree add -q --orphan -b gh-pages "$work"
  fi
fi
updated="$(date -u '+%Y-%m-%d %H:%M UTC')"
sed -e "s#__LIVE_URL__#${live}#g" -e "s#__UPDATED__#${updated}#g" site/index.html > "$work/index.html"
cp demo/quarantinepay-autonomous-demo.mp4 demo/quarantinepay-agent-demo.mp4 "$work/"
# WebVTT captions for the HTML5 <track> element.
{ echo "WEBVTT"; echo; sed -E 's/([0-9]{2}:[0-9]{2}:[0-9]{2}),([0-9]{3})/\1.\2/g' demo/quarantinepay-autonomous-demo.en.srt; } > "$work/quarantinepay-autonomous-demo.en.vtt"
cp docs/evidence/1-agent-proposal.png docs/evidence/2-injection-policy-hold.png \
   docs/evidence/3-clean-delivery-captured.png docs/evidence/4-intelligence-grid.png "$work/"
ffmpeg -loglevel error -y -ss 46 -i demo/quarantinepay-autonomous-demo.mp4 -frames:v 1 "$work/poster.png"
touch "$work/.nojekyll"
printf '{"live_url":"%s","updated":"%s"}\n' "$live" "$updated" > "$work/live.json"
git -C "$work" add -A
if git -C "$work" diff --cached --quiet; then echo "Pages already up to date"; exit 0; fi
git -C "$work" -c user.name="$(git config user.name)" -c user.email="$(git config user.email)" \
  commit -q -m "Publish showcase; live demo at ${live}"
git -C "$work" push -q origin gh-pages
echo "Published Pages pointing at ${live}"
