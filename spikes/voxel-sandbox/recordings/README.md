# Recordings

Scripts that drive the sandbox in a headless browser and encode a video of a cropped view.
They need the dev server running (`npx vite --port 5173`) plus Playwright and ffmpeg.

    node recordings/record.cjs recordings/<scenario>.cjs <out-prefix>

A scenario lists the commands to schedule, the crop to record and, for setup only, boxes of
material to place (`build`) or to dry out (`dry`) before the first tick.

- `grass-post.cjs`: grass fire by the dry post, 30 s.
- `living-tree.cjs`: a crib of dry logs against the big north tree, 90 s.
- `dead-tree.cjs`: the same with the tree dried to 10% moisture, 90 s.
