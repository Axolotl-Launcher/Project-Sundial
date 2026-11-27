import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, extname, join, resolve } from "node:path";
import { createInterface } from "node:readline";

const root = resolve(import.meta.dirname, "..");
const NODE_ARGUMENT_OFFSET = 2;
const platforms: Partial<Record<NodeJS.Platform, string>> = {
  win32: "windows",
  linux: "linux",
  darwin: "macos",
};
const extensions: Partial<Record<NodeJS.Platform, string>> = {
  win32: ".dll",
  linux: ".so",
  darwin: ".dylib",
};

interface Artifact {
  reason?: string;
  target?: { name?: string };
  filenames?: string[];
  executable?: string;
  message?: { rendered?: string };
}

async function cargoArtifact(
  args: string[],
  name: string,
  output: (message: Artifact) => string | undefined,
  env: NodeJS.ProcessEnv = process.env,
): Promise<string> {
  const cargo = spawn("cargo", [...args, "--message-format=json-render-diagnostics"], {
    cwd: root, env, stdio: ["inherit", "pipe", "inherit"],
  });
  const exit = new Promise<number | null>((resolveExit, reject) => {
    cargo.once("error", reject);
    cargo.once("close", resolveExit);
  });
  let artifact: string | undefined;
  const lines = createInterface({ input: cargo.stdout });
  for await (const line of lines) {
    const message = JSON.parse(line) as Artifact;
    if (message.message?.rendered) process.stderr.write(message.message.rendered);
    if (message.reason === "compiler-artifact" && message.target?.name === name) {
      artifact = output(message) ?? artifact;
    }
  }
  if (await exit !== 0 || !artifact) throw new Error(`Cargo did not produce ${name}`);
  return artifact;
}

function library(name: string, profile: string): Promise<string> {
  const extension = extensions[process.platform];
  const features = name === "axlauncher_download_engine" ? ["--features", "worker"] : [];
  return cargoArtifact(
    ["rustc", "--locked", "--profile", profile, "-p", name, "--lib", "--crate-type", "cdylib", ...features],
    name,
    ({ filenames }) => filenames?.find((file) => extname(file) === extension),
  );
}

function exec(program: string, args: string[], env = process.env): number {
  const result = spawnSync(program, args, { cwd: root, env, stdio: "inherit" });
  if (result.error) throw result.error;
  return result.status ?? 1;
}

function sign(image: string, identity: string, staging: string): string {
  const copy = join(staging, basename(image));
  copyFileSync(image, copy);
  if (exec("codesign", ["--force", "--sign", identity, copy]) !== 0
      || exec("codesign", ["--verify", "--strict", copy]) !== 0) {
    throw new Error(`Could not sign ${basename(image)}`);
  }
  return copy;
}

async function main(args: string[]): Promise<number> {
  const [mode, ...rest] = args;
  const platform = platforms[process.platform];
  if (!platform) throw new Error(`Unsupported desktop host: ${process.platform}`);
  const desktop = `axlauncher_platform_${platform}`;
  const profile = mode === "build" || rest.includes("--release") ? "release" : "dev";
  const commands: Record<string, string[]> = {
    build: ["build", "--locked", "--release", "-p", desktop],
    dev: ["run", "--locked", "-p", desktop],
    lint: ["clippy", "--locked", "-p", desktop, "--all-targets", "--", "-D", "warnings"],
    test: ["test", "--locked", "-p", desktop],
  };
  if (!mode || !(mode in commands || mode === "exec" || mode === "smoke")) {
    throw new Error("Usage: node scripts/desktop.ts <build|dev|lint|test|smoke|exec command ...>");
  }
  let downloads = await library("axlauncher_download_engine", profile);
  let accounts = await library("axlauncher_accounts", profile);
  let javaDiscovery = await library("axlauncher_java_discovery", profile);
  let instanceSettings = await library("axlauncher_instance_settings", profile);
  const identity = process.platform === "darwin" ? process.env.AXLAUNCHER_MACOS_SIGN_IDENTITY : undefined;
  const staging = identity ? mkdtempSync(join(tmpdir(), "axlauncher-sign-")) : undefined;
  try {
    if (identity && staging) {
      downloads = sign(downloads, identity, staging);
      accounts = sign(accounts, identity, staging);
      javaDiscovery = sign(javaDiscovery, identity, staging);
      instanceSettings = sign(instanceSettings, identity, staging);
    }
    const env = {
      ...process.env,
      AXLAUNCHER_DOWNLOAD_IMAGE: downloads,
      AXLAUNCHER_ACCOUNT_IMAGE: accounts,
      AXLAUNCHER_JAVA_DISCOVERY_IMAGE: javaDiscovery,
      AXLAUNCHER_INSTANCE_SETTINGS_IMAGE: instanceSettings,
    };
    if (mode === "exec") {
      const [command, ...parameters] = rest;
      if (!command) throw new Error("Provide the command to run with embedded worker images");
      return exec(command, parameters, env);
    }
    if (mode === "smoke") {
      const executable = await cargoArtifact(
        ["build", "--locked", "--profile", profile, "-p", desktop, "--bin", "axlauncher"],
        "axlauncher",
        ({ executable }) => executable,
        env,
      );
      return exec("cargo", [
        "test", "--locked", "-p", "axlauncher_policy_tests", "--test", "embedded_workers", "--", "--ignored",
        ...rest.filter((argument) => argument !== "--release"),
      ], { ...env, AXLAUNCHER_EXECUTABLE: executable, AXLAUNCHER_SMOKE_EXE: executable });
    }
    const command = commands[mode];
    if (!command) throw new Error(`Unknown desktop command: ${mode}`);
    return exec("cargo", [...command, ...rest], env);
  } finally {
    if (staging) rmSync(staging, { recursive: true, force: true });
  }
}

main(process.argv.slice(NODE_ARGUMENT_OFFSET)).then((result) => {
  process.exitCode = result;
}).catch((error: unknown) => {
  console.error(error);
  process.exitCode = 1;
});
