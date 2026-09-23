const { buildSync } = require('esbuild');
const { mkdirSync, copyFileSync } = require('node:fs');
mkdirSync('build', { recursive: true });
buildSync({ entryPoints: ['src/renderer.js'], bundle: true, outfile: 'build/renderer.js', platform: 'browser' });
for (const file of ['index.html', 'style.css']) copyFileSync(`src/${file}`, `build/${file}`);
