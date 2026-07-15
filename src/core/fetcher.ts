function getDecompressor(path: string): string | null {
  if (path.endsWith(".zst")) return "zstdcat";
  if (path.endsWith(".gz")) return "zcat";
  if (path.endsWith(".bz2")) return "bzcat";
  if (path.endsWith(".xz")) return "xzcat";
  return null;
}

async function runCommand(
  cmd: string[],
  options: { stdin?: ReadableStream<Uint8Array> } = {}
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  const proc = Bun.spawn(cmd, {
    stdin: options.stdin ?? "inherit",
    stdout: "pipe",
    stderr: "pipe",
  });

  const stdout = await new Response(proc.stdout).text();
  const stderr = await new Response(proc.stderr).text();
  const exitCode = await proc.exited;

  return { stdout, stderr, exitCode };
}

async function locateManPage(topic: string): Promise<string | null> {
  const { stdout, exitCode } = await runCommand(["man", "-w", topic]);
  if (exitCode !== 0) return null;

  const firstPath = stdout.trim().split("\n")[0];
  return firstPath ?? null;
}

async function renderWithMandoc(path: string): Promise<string> {
  const decompressor = getDecompressor(path);

  if (decompressor) {
    const decompressProc = Bun.spawn([decompressor, path], {
      stdout: "pipe",
      stderr: "pipe",
    });

    const mandocProc = Bun.spawn(["mandoc", "-Thtml"], {
      stdin: decompressProc.stdout,
      stdout: "pipe",
      stderr: "pipe",
    });

    const html = await new Response(mandocProc.stdout).text();
    const mandocErr = await new Response(mandocProc.stderr).text();
    const decompressErr = await new Response(decompressProc.stderr).text();
    const mandocCode = await mandocProc.exited;
    const decompressCode = await decompressProc.exited;

    if (decompressCode !== 0 || mandocCode !== 0) {
      throw new Error(
        (mandocErr || decompressErr || "mandoc pipeline failed").trim()
      );
    }

    return html;
  }

  const { stdout, stderr, exitCode } = await runCommand([
    "mandoc",
    "-Thtml",
    path,
  ]);

  if (exitCode !== 0) {
    throw new Error(stderr.trim() || `mandoc -Thtml ${path} failed`);
  }

  return stdout;
}

async function renderWithMan(topic: string): Promise<string> {
  const { stdout, stderr, exitCode } = await runCommand([
    "man",
    "-Thtml",
    topic,
  ]);

  if (exitCode !== 0) {
    throw new Error(
      stderr.trim() || `man -Thtml ${topic} failed with code ${exitCode}`
    );
  }

  return stdout;
}

export async function fetchManHtml(topic: string): Promise<string> {
  const path = await locateManPage(topic);

  if (path) {
    try {
      const html = await renderWithMandoc(path);
      if (html.trim().length > 0) {
        return html;
      }
    } catch (err) {
      // Fall back to `man -Thtml` if mandoc fails for any reason.
      const message = err instanceof Error ? err.message : String(err);
      console.warn(`mandoc failed for ${topic}, falling back to man: ${message}`);
    }
  }

  return renderWithMan(topic);
}
