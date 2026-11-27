// Writes gzip and Brotli copies of the admin script and stylesheet next to the build output.
// The Rust binary embeds them and serves the smallest encoding the browser accepts.
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';
import { readFileSync, writeFileSync } from 'node:fs';

for (const file of ['dist/assets/admin.js', 'dist/assets/admin.css']) {
  const data = readFileSync(file);
  writeFileSync(`${file}.gz`, gzipSync(data, { level: 9 }));
  writeFileSync(
    `${file}.br`,
    brotliCompressSync(data, { params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: data.length } }),
  );
}
