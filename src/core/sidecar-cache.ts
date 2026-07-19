/**
 * @file Materializes the compiled executable's embedded libmandoc sidecar.
 *
 * Bun keeps compiled assets in its internal virtual file system. Native
 * executables must instead live on an executable filesystem path, so Mant
 * writes the embedded sidecar once into a private per-user cache and reuses it.
 */

import { rmSync } from "node:fs";
import { access, chmod, mkdir, mkdtemp, rename, rm } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";

const SIDECAR_ASSET_BASENAME = "mant-mandoc-json";
const SIDECAR_FILENAME = SIDECAR_ASSET_BASENAME;

let materializedSidecar: Promise<string> | null = null;

declare const MANT_COMPILED: boolean;

// Bun's declaration uses Blob, while compiled-file assets additionally carry
// their generated asset name at runtime (as documented by Bun.embeddedFiles).
interface EmbeddedAsset extends Blob {
  name: string;
}

/**
 * Resolves a user-writable cache root. MANT_SIDECAR_DIR is intentionally a
 * directory override, while MANT_MANDOC_JSON_BIN remains a direct binary-path
 * override in the roff AST fetcher.
 */
export function getSidecarCacheDir(
  env: Record<string, string | undefined> = process.env,
  platform: string = process.platform,
): string {
  if (env.MANT_SIDECAR_DIR) return env.MANT_SIDECAR_DIR;

  const home = env.HOME ?? homedir();
  if (platform === "darwin") {
    return join(home, "Library", "Caches", "mant", "sidecars");
  }
  return join(
    env.XDG_CACHE_HOME ?? join(home, ".cache"),
    "mant",
    "sidecars",
  );
}

/** Resolves the override, embedded asset, or development sidecar path. */
export function getBundledSidecarPath(): string | Promise<string> {
  if (process.env.MANT_MANDOC_JSON_BIN) {
    return process.env.MANT_MANDOC_JSON_BIN;
  }
  if (typeof MANT_COMPILED !== "undefined" && MANT_COMPILED) {
    return materializeEmbeddedSidecar();
  }
  return new URL("../../native/bin/mant-mandoc-json", import.meta.url).pathname;
}

function findEmbeddedSidecar(): EmbeddedAsset {
  const sidecar = (Bun.embeddedFiles as readonly EmbeddedAsset[]).find((file) =>
    file.name === SIDECAR_ASSET_BASENAME
    || file.name.startsWith(`${SIDECAR_ASSET_BASENAME}-`),
  );
  if (!sidecar) {
    throw new Error(
      "compiled Mant executable does not contain the libmandoc sidecar; rebuild with bun run build",
    );
  }
  return sidecar;
}

async function isExecutable(path: string): Promise<boolean> {
  try {
    await access(path, 0o1);
    return true;
  } catch {
    return false;
  }
}

async function writeSidecar(directory: string, sidecar: EmbeddedAsset): Promise<string> {
  // The asset name has Bun's content hash, so rebuilding with new native code
  // naturally selects a separate cache entry instead of reusing stale bytes.
  await mkdir(directory, { recursive: true, mode: 0o700 });
  await chmod(directory, 0o700);
  const assetDirectory = join(directory, sidecar.name);
  const target = join(assetDirectory, SIDECAR_FILENAME);
  if (await isExecutable(target)) return target;

  await mkdir(assetDirectory, { recursive: true, mode: 0o700 });
  await chmod(assetDirectory, 0o700);
  if (await isExecutable(target)) return target;

  const stagingDirectory = await mkdtemp(join(assetDirectory, ".extract-"));
  const stagingPath = join(stagingDirectory, SIDECAR_FILENAME);
  try {
    await Bun.write(stagingPath, sidecar);
    await chmod(stagingPath, 0o700);
    try {
      await rename(stagingPath, target);
    } catch (error) {
      // Another Mant process may have completed the same extraction first.
      if (!(await isExecutable(target))) throw error;
    }
  } finally {
    await rm(stagingDirectory, { recursive: true, force: true });
  }
  return target;
}

async function writeTemporarySidecar(sidecar: EmbeddedAsset): Promise<string> {
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "mant-sidecar-"));
  const target = join(temporaryDirectory, SIDECAR_FILENAME);
  try {
    await Bun.write(target, sidecar);
    await chmod(target, 0o700);
    return target;
  } catch (error) {
    await rm(temporaryDirectory, { recursive: true, force: true });
    throw error;
  }
}

function assertSidecarCanRun(path: string): void {
  const result = Bun.spawnSync([path, "--help"], {
    stdout: "ignore",
    stderr: "ignore",
  });
  if (result.exitCode !== 0) {
    throw new Error(
      `${path} was extracted but cannot run; its filesystem may be mounted noexec`,
    );
  }
}

async function materialize(): Promise<string> {
  const sidecar = findEmbeddedSidecar();
  try {
    const path = await writeSidecar(getSidecarCacheDir(), sidecar);
    assertSidecarCanRun(path);
    return path;
  } catch (cacheError) {
    if (process.env.MANT_DEBUG) {
      console.warn(`could not cache embedded libmandoc sidecar: ${String(cacheError)}`);
    }
    try {
      const path = await writeTemporarySidecar(sidecar);
      try {
        assertSidecarCanRun(path);
      } catch (error) {
        await rm(dirname(path), { recursive: true, force: true });
        throw error;
      }
      // A temporary fallback is useful only for this process. Cache extraction
      // is preferred because it avoids leaving an executable under /tmp.
      process.once("exit", () => rmSync(dirname(path), { recursive: true, force: true }));
      return path;
    } catch (temporaryError) {
      throw new Error(
        "could not materialize the embedded libmandoc sidecar; "
        + `cache error: ${String(cacheError)}; temporary-directory error: ${String(temporaryError)}. `
        + "Set MANT_SIDECAR_DIR to a writable executable directory or MANT_MANDOC_JSON_BIN to an external sidecar.",
      );
    }
  }
}

/** Materializes the embedded sidecar once per process and returns its path. */
export function materializeEmbeddedSidecar(): Promise<string> {
  materializedSidecar ??= materialize();
  return materializedSidecar;
}
