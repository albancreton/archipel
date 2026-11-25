F=/work/t3code-minitel/apps/server/src/orchestration-v2/testkit/fixtures
while read -r name mode policy; do
  echo "=================== $name"
  node run-claude.mts "$F/$name/claude_transcript.ndjson" "$mode" "$policy" 2>&1 | cut -c1-700 | tail -${TAIL:-60}
done
