(() => {
  const data = JSON.parse(document.getElementById("synth-artifact-data").textContent);
  const root = document.getElementById("app");
  root.innerHTML = "";
  const header = document.createElement("header");
  const kicker = document.createElement("p");
  kicker.className = "kicker";
  kicker.textContent = data.template_id;
  const title = document.createElement("h1");
  title.textContent = data.title;
  header.append(kicker, title);
  const section = document.createElement("section");
  section.className = "visual";
  root.append(header, section);
  const projection = window.SynthRolloutInspector
    && window.SynthRolloutInspector.extractProjection(data.bindings);
  if (data.template_id === "trace.rollout_inspector.v1" && projection) {
    window.SynthRolloutInspector.mount(section, {
      traces: [{ label: data.title, projection }],
    });
    return;
  }
  const pre = document.createElement("pre");
  pre.textContent = JSON.stringify(data.bindings, null, 2);
  section.append(pre);
})();
