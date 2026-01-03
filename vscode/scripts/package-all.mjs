#!/usr/bin/env node
// Build and package all VS Code extension variants (platform-specific + web).

import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { promises as fsp } from "node:fs";

const TARGETS = [
  { vsce: "win32-x64", triplets: ["x86_64-pc-windows-msvc"] },
  { vsce: "darwin-arm64", triplets: ["aarch64-apple-darwin"] },
  { vsce: "linux-x64", triplets: ["x86_64-unknown-linux-gnu"] },
  { vsce: "linux-arm64", triplets: ["aarch64-unknown-linux-gnu"] },
  { vsce: "web", triplets: [], noBundle: true },
];

const BINARY_BY_VSCE = new Map([
  ["win32-x64", "beancount-language-server.exe"],
  ["darwin-arm64", "beancount-language-server"],
  ["linux-x64", "beancount-language-server"],
  ["linux-arm64", "beancount-language-server"],
]);

const ROOT = path.resolve(
  path.join(fileURLToPath(new URL(".", import.meta.url)), ".."),
);
const STASH_DIR = path.join(ROOT, ".cache", "package-all");

function run(cmd, args, env = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, {
      stdio: "inherit",
      shell: process.platform === "win32",
      env: { ...process.env, ...env },
      cwd: ROOT,
    });
    child.on("exit", (code) => {
      if (code === 0) return resolve();
      reject(new Error(`${cmd} ${args.join(" ")} exited with ${code}`));
    });
  });
}

async function packageTarget(target) {
  const env = {
    VSCE_TARGET: target.vsce,
    GITHUB_TOKEN: process.env.GITHUB_TOKEN,
  };

  // Start from a clean server directory so we only bundle the target's binary.
  await fsp.rm(path.join(ROOT, "server"), { recursive: true, force: true });

  if (!target.noBundle && target.triplets.length) {
    const binaryName =
      BINARY_BY_VSCE.get(target.vsce) || "beancount-language-server";
    for (const triplet of target.triplets) {
      const srcFile = path.join(STASH_DIR, triplet, binaryName);
      const destDir = path.join(ROOT, "server", triplet);
      await fsp.mkdir(destDir, { recursive: true });
      const destFile = path.join(destDir, binaryName);
      await fsp.copyFile(srcFile, destFile);
    }
  }

  const distDir = path.join(
    path.resolve(path.join(fileURLToPath(new URL(".", import.meta.url)), "..")),
    "dist",
  );
  await fsp.mkdir(distDir, { recursive: true });

  const output = path.join(
    "dist",
    `beancount-language-server-${target.vsce}.vsix`,
  );

  const publishArgs = [
    "exec",
    "vsce",
    "package",
    "--no-dependencies",
    "--target",
    target.vsce,
    "-o",
    output,
  ];

  await run("pnpm", publishArgs, env);
}

async function main() {
  const combinedTriplets = [
    ...new Set(
      TARGETS.filter((t) => !t.noBundle)
        .flatMap((t) => t.triplets)
        .filter(Boolean),
    ),
  ];

  if (combinedTriplets.length) {
    await run("pnpm", ["run", "download:binaries"], {
      ...process.env,
      BLS_TARGETS: combinedTriplets.join(","),
    });

    const binaryNameDefault = "beancount-language-server";
    await fsp.mkdir(STASH_DIR, { recursive: true });
    for (const triplet of combinedTriplets) {
      const binaryName =
        BINARY_BY_VSCE.get(
          TARGETS.find((t) => t.triplets.includes(triplet))?.vsce || "",
        ) || binaryNameDefault;
      const srcFile = path.join(ROOT, "server", triplet, binaryName);
      const destDir = path.join(STASH_DIR, triplet);
      await fsp.mkdir(destDir, { recursive: true });
      await fsp.copyFile(srcFile, path.join(destDir, binaryName));
    }

    await fsp.rm(path.join(ROOT, "server"), { recursive: true, force: true });
  }

  await run("pnpm", ["run", "build-base"], process.env);

  for (const target of TARGETS) {
    console.log(`\n=== Packaging target ${target.vsce} ===`);
    await packageTarget(target);
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
