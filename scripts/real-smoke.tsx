import { createTestRenderer } from "@opentui/core/testing";
import { createRoot } from "@opentui/react";
import { query } from "../src/query";
import { App } from "../src/ui/app";

async function main() {
  const result = await query({ topic: "ls" });

  const setup = await createTestRenderer({
    width: 80,
    height: 24,
    bufferedOutput: "memory",
  });

  const root = createRoot(setup.renderer);
  root.render(<App result={result} onQuit={() => setup.renderer.destroy()} />);

  await new Promise((resolve) => setTimeout(resolve, 200));

  const frame = setup.captureCharFrame();
  console.log("=== FRAME ===");
  console.log(frame);
  console.log("=== END FRAME ===");

  const visibleChars = frame.replace(/\s/g, "").length;
  console.log(`visibleChars: ${visibleChars}`);

  setup.renderer.destroy();
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
