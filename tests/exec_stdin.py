import json
import pathlib
import subprocess
import sys
import tempfile
import uuid


def run(*args):
    return subprocess.check_output(args, text=True, timeout=90)


def main():
    binary = str(pathlib.Path(sys.argv[1]).resolve())
    name = "exec-stdin-" + uuid.uuid4().hex[:8]
    with tempfile.TemporaryDirectory(prefix="herdr-exec-stdin-") as directory:
        mod = pathlib.Path(directory)
        (mod / "mod.json").write_text(json.dumps({"place": "none"}))
        (mod / "main.js").write_text('''import { View, div } from "gpui-kit";
import { exec } from "herdr";
let result = "WAITING";
export default class StdinCheck extends View {
  init(_props, cx) {
    cx.spawn(async cx => {
      try {
        const read = ["-e", 'let s="";process.stdin.setEncoding("utf8");process.stdin.on("data",d=>s+=d);process.stdin.on("end",()=>process.stdout.write(s))'];
        const input = "fixture-ä-🔒\\n".repeat(16384);
        const withInput = await exec("node", read, {stdin: input});
        const emptyInput = await exec("node", read, {stdin: ""});
        const withoutInput = await exec("node", read);
        result = withInput.code === 0 && withInput.stdout === input && emptyInput.code === 0 && emptyInput.stdout === "" && withoutInput.code === 0 && withoutInput.stdout === "" ? "EXEC_STDIN_PASS" : "EXEC_STDIN_FAIL";
      } catch (error) {
        result = "EXEC_STDIN_FAIL: " + String(error);
      }
      cx.notify();
    });
  }
  render() { return div().child(result); }
}
''')
        try:
            run("herdr-ctrl", "spawn", name, "--fresh", "--ttl", "5m", "--bin", binary, "--mod", directory)
            run("herdr-ctrl", "--on", name, "do", "mod.show", "name=" + mod.name)
            run("herdr-ctrl", "--on", name, "wait", "--text", "EXEC_STDIN_PASS")
            status = json.loads(run("herdr-ctrl", "--on", name, "status"))
            assert not status["errors"], status["errors"]
            print("Native exec stdin, Unicode, pipe-sized input and EOF passed")
        except Exception:
            print(run("herdr-ctrl", "--on", name, "snapshot"))
            print(run("herdr-ctrl", "--on", name, "status"))
            raise
        finally:
            subprocess.run(["herdr-ctrl", "stop", name], check=True, timeout=90)


if __name__ == "__main__":
    main()
