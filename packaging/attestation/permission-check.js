// Permission regression gate; no remote code or real credentials are used.
const checks = {};
const actions = {
  read: () => Deno.readTextFile("permission-canary.txt"),
  write: () => Deno.writeTextFile("permission-canary.txt", "unexpected"),
  net: () => fetch("https://www.youtube.com/"),
  env: () => Deno.env.get("PATH"),
  run: () => new Deno.Command("cmd.exe", { args: ["/c", "exit"] }).output(),
  ffi: () => Deno.dlopen("kernel32.dll", {}),
  sys: () => Deno.hostname(),
};
for (const [name, action] of Object.entries(actions)) {
  try {
    await action();
    checks[name] = false;
  } catch (error) {
    checks[name] = error instanceof Deno.errors.NotCapable;
  }
}
console.log(JSON.stringify(checks));
if (Object.values(checks).some((denied) => !denied)) Deno.exit(1);
