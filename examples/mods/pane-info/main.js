// A pane sidebar tab that shows facts about the pane it is attached to.
// Open it on the focused pane:   herdr-app do pane.sidebar.open --mod pane-info
// or from a mod or key binding:  run("pane.sidebar.toggle", { mod: "pane-info" })
// Props from the app: { pane, terminal, sidebar } plus any props passed to open.

import { View, div } from "gpui-kit";
import { v_flex, h_flex } from "gpui-base";
import { panes, props, run, theme, nextCommand } from "herdr";

export default class PaneInfo extends View {
  init(_props, cx) {
    this.handle = cx.focus_handle();
    cx.spawn(async (cx) => {
      for (;;) {
        if ((await nextCommand()) === "focus") this.handle.focus();
        cx.notify();
      }
    });
  }

  close() {
    run("pane.sidebar.close", { pane: props().pane, id: props().sidebar });
  }

  button(t, id, label, action) {
    return div()
      .id(id)
      .px(8)
      .py(4)
      .rounded(5)
      .bg(t.raised)
      .hover((s) => s.bg(t.hover))
      .on_click(() => action())
      .child(label);
  }

  render() {
    const t = theme();
    const me = panes().find((p) => p.pane === props().pane);
    const row = (k, v) => h_flex().gap(8).child(div().w(64).text_color(t.dim).child(k)).child(div().child(String(v ?? "–")));
    return v_flex()
      .id("pane-info")
      .track_focus(this.handle)
      .size_full()
      .p(12)
      .gap(6)
      .text_size(12)
      .text_color(t.foreground)
      .on_key_down((e, cx) => {
        if (e.key !== "x") return;
        this.close();
        cx.stop_propagation();
      })
      .child(row("title", me?.title))
      .child(row("folder", me?.cwd))
      .child(row("branch", me?.branch))
      .child(row("terminal", props().terminal))
      .child(row("tab", props().sidebar))
      .child(row("note", props().note))
      .child(
        h_flex()
          .gap(6)
          .pt(8)
          .child(this.button(t, "pane-info-back", "Back to terminal", () => run("pane.sidebar.focus", { pane: props().pane })))
          .child(this.button(t, "pane-info-close", "Close (x)", () => this.close())),
      );
  }
}
