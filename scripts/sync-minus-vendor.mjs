#!/usr/bin/env node
// Reproduce the private static/search pager module from the pinned crate archive.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const [source, flag] = process.argv.slice(2);
if (!source || !['--verify', '--patch'].includes(flag)) throw new Error('usage: sync-minus-vendor.mjs EXTRACTED_MINUS_5_7_2 --verify|--patch');
const manifest = fs.readFileSync(path.join(source, 'Cargo.toml'), 'utf8');
if (!manifest.includes('version = "5.7.2"')) throw new Error('expected minus 5.7.2');
const destination = path.join(root, 'crates/mant-ui/src/pager/vendor');

export function adapted(name, input) {
  let text = input.replaceAll('crate::', 'crate::pager::native::');
  text = text.replace(/feature = "(search|static_output)"/g, 'all()')
    .replace(/feature = "(dynamic_output|clipboard)"/g, 'any()')
    .replaceAll('any(any(), all())', 'all()')
    .replaceAll('all(all(), not(test))', 'not(test)')
    .replace(/^#!?\[cfg_attr\(docsrs,.*\)\]\n/gm, '')
    .replace(/^[ \t]*#\[cfg\(all\(\)\)\]\n/gm, '')
    .replaceAll('#[cfg(all())] ', '');
  if (name === 'lib.rs') {
    text = text.replace(/^#!\[(deny|warn)\(clippy::\w+\)\]\n/gm, '')
      .replace(/^#!\[cfg_attr\(doctest,.*\)\]\n/gm, '');
  }
  if (name === 'screen/mod.rs') {
    text = text.replace('    let (last_idx, last_line_text)', '    let mut sgr = crate::pager::sgr::SgrState::default();\n    let (last_idx, last_line_text)')
      .replace('        let rows = format_line(\n            line,', '        let logical_line = sgr.logical_line(line);\n        let rows = format_line(\n            &logical_line,')
      .replace('    let last_line = format_line(\n        last_line_text,', '    let logical_line = sgr.logical_line(last_line_text);\n    let last_line = format_line(\n        &logical_line,');
    const needle = '    let enumerated_rows = if line_wrapping {';
    if (!text.includes(needle)) throw new Error('upstream wrapping boundary changed');
    text = text.replace(needle, '    let wrapped_rows = if line_wrapping {');
    const end = '    }\n    .into_iter()\n    .enumerate();';
    if (!text.includes(end)) throw new Error('upstream row iterator changed');
    text = text.replace(end, '    };\n    let enumerated_rows = crate::pager::sgr::independent_rows(wrapped_rows).into_iter().enumerate();');
  }
  if (name === 'core/utils/display/tests.rs') text = text.replace('let res = Vec::new();', 'let res: Vec<u8> = Vec::new();').replace('res.contains("minus")', 'res.contains("mant_ui")');
  return text;
}

let checked = 0;
const patch = ['*** Begin Patch'];
function check(name, expected) {
  expected = expected.trimEnd() + '\n';
  if (flag === '--patch') {
    patch.push(`*** Add File: ${path.join(destination, name)}`, ...expected.trimEnd().split('\n').map(line => `+${line}`));
  } else if (fs.readFileSync(path.join(destination, name), 'utf8') !== expected) throw new Error(`vendor differs: ${name}`);
}
function verifyTree(relative = '') {
  for (const entry of fs.readdirSync(path.join(source, 'src', relative), { withFileTypes: true })) {
    const name = path.posix.join(relative, entry.name);
    if (entry.isDirectory()) verifyTree(name);
    else {
      const expected = adapted(name, fs.readFileSync(path.join(source, 'src', name), 'utf8'));
      check(name, expected);
      checked++;
    }
  }
}
verifyTree();
for (const name of ['LICENSE-APACHE', 'LICENSE-MIT']) {
  check(name, fs.readFileSync(path.join(source, name), 'utf8'));
}
patch.push('*** End Patch');
process.stdout.write(flag === '--patch' ? patch.join('\n') : `verified minus 5.7.2 private module (${checked} source files)\n`);
