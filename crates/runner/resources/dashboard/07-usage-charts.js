function drawLedgerTrend() {
  const box = document.getElementById("ledChart");
  if (!box || !usageDigest) return;
  const d = usageDigest;
  const bucket = ledBucketName(d);
  box.replaceChildren();
  const plot = document.createElement("div");
  plot.className = "plot";
  box.append(plot);
  const W = plot.clientWidth;
  const H = plot.clientHeight || 236;
  const pts = d.series || [];
  if (W < 100 || !pts.length) {
    const none = document.createElement("div");
    none.className = "none";
    none.textContent = "本周期没有请求";
    plot.append(none);
    drawLedgerRank(d, bucket);
    return;
  }
  const { top } = ledCategories(ledSplit, ledMetric);
  const totals = pts.map((p) => ledValue(ledMetric, p));
  const speed = ledMetric === "speed";
  const partOf = (p, id) => (p.by && p.by[ledSplit] && p.by[ledSplit][id]) || {};
  const max = Math.max(
    0,
    ...totals,
    ...(speed ? pts.flatMap((p) => top.map((c) => ledFloor(partOf(p, c.id)))) : []),
  );
  if (!(max > 0)) {
    const none = document.createElement("div");
    none.className = "none";
    none.textContent =
      ledMetric === "cost"
        ? "本周期没有已知价格的请求"
        : speed
          ? "本周期没有可整轮计时的回复"
          : "本周期没有请求";
    plot.append(none);
    drawLedgerRank(d, bucket);
    return;
  }
  const topV = ledAxis(max);
  const g = sv("svg", { viewBox: `0 0 ${W} ${H}`, role: "img" });
  g.setAttribute("aria-label", "用量趋势");
  plot.append(g);
  const labels = [0, 1, 2, 3, 4].map((i) => {
    const lab = sv("text", { "text-anchor": "end", class: "axis" });
    lab.textContent = ledAxisFormat(ledMetric, topV * (1 - i / 4));
    g.append(lab);
    return lab;
  });
  let widest = 0;
  labels.forEach((lab) => {
    try {
      widest = Math.max(widest, lab.getComputedTextLength());
    } catch (e) {}
  });
  const M = { l: widest > 0 ? Math.ceil(widest) + 8 : 48, r: 8, t: 10, b: 24 };
  const pw = W - M.l - M.r;
  const ph = H - M.t - M.b;
  const n = pts.length;
  const slot = pw / n;
  const bw = Math.max(2, Math.min(30, slot * 0.68));
  const Y = (v) => M.t + ph - (ph * v) / topV;
  labels.forEach((lab, i) => {
    const y = M.t + (ph * i) / 4;
    lab.setAttribute("x", M.l - 6);
    lab.setAttribute("y", y + 3.5);
    g.insertBefore(
      sv("line", { x1: M.l, x2: W - M.r, y1: y, y2: y, class: "grid" }),
      lab,
    );
  });
  const room = Math.max(2, Math.floor(pw / 62));
  const every = Math.ceil(n / room);
  pts.forEach((p, i) => {
    if (i % every) return;
    const x = sv("text", {
      x: M.l + slot * (i + 0.5),
      y: H - 6,
      "text-anchor": "middle",
      class: "axis",
    });
    x.textContent = ledTick(p.t, bucket);
    g.append(x);
  });
  const hot = sv(
    "rect",
    { class: "hot", y: M.t, height: ph, rx: 3, width: slot },
    { display: "none" },
  );
  g.append(hot);

  const segs = [];
  pts.forEach((p, i) => {
    const x = M.l + slot * (i + 0.5) - bw / 2;
    let y0 = 0;
    const draw = (key, color, v) => {
      if (!(v > 0)) return;
      const r = sv(
        "rect",
        {
          x,
          width: bw,
          y: Y(y0 + v),
          height: Math.max(0.5, Y(y0) - Y(y0 + v)),
          rx: 1,
          class: "col",
          "data-k": key,
          "data-color": color,
        },
        { fill: color },
      );
      g.append(r);
      segs.push(r);
      y0 += v;
    };
    if (speed) {
      // Grouped bars: each active model in this bucket gets its own bar at its true speed
      const active = top
        .map((c) => ({ ...c, v: ledFloor(partOf(p, c.id)) }))
        .filter((c) => c.v > 0);
      const K = active.length;
      if (K === 0) return;
      if (K === 1) {
        const c = active[0];
        const barW = bw;
        const barX = M.l + slot * (i + 0.5) - barW / 2;
        const r = sv(
          "rect",
          {
            x: barX,
            width: barW,
            y: Y(c.v),
            height: Math.max(0.5, Y(0) - Y(c.v)),
            rx: 1,
            class: "col",
            "data-k": c.id,
            "data-color": c.color,
          },
          { fill: c.color },
        );
        g.append(r);
        segs.push(r);
      } else {
        const gap = Math.min(2, Math.max(1, slot * 0.04));
        const totalW = Math.min(slot * 0.85, bw * 1.4);
        const barW = Math.max(2, (totalW - gap * (K - 1)) / K);
        const groupW = K * barW + (K - 1) * gap;
        const startX = M.l + slot * (i + 0.5) - groupW / 2;
        active.forEach((c, idx) => {
          const barX = startX + idx * (barW + gap);
          const r = sv(
            "rect",
            {
              x: barX,
              width: barW,
              y: Y(c.v),
              height: Math.max(0.5, Y(0) - Y(c.v)),
              rx: 1,
              class: "col",
              "data-k": c.id,
              "data-color": c.color,
            },
            { fill: c.color },
          );
          g.append(r);
          segs.push(r);
        });
      }
      return;
    }
    let stacked = 0;
    for (const c of top) {
      const v = ledPartValue(ledMetric, partOf(p, c.id));
      stacked += v;
      draw(c.id, c.color, v);
    }
    draw("\0other", "var(--faint)", Math.max(0, totals[i] - stacked));
  });

  const tip = document.createElement("div");
  tip.className = "tip";
  tip.hidden = true;
  plot.append(tip);
  const nameOf = (c) => ledName(c, ledSplit);
  const at = (e) => {
    const r = g.getBoundingClientRect();
    const i = Math.max(
      0,
      Math.min(
        n - 1,
        Math.floor((((e.clientX - r.left) / r.width) * W - M.l) / slot),
      ),
    );
    const p = pts[i];
    hot.setAttribute("x", M.l + slot * i);
    hot.style.display = "";
    tip.replaceChildren();
    const head = document.createElement("b");
    head.textContent = ledWhen(p.t, bucket);
    tip.append(head);
    const rows = top.map((c) => {
      const part = partOf(p, c.id);
      return [nameOf(c), c.color, ledPartValue(ledMetric, part)];
    });
    if (!speed) {
      rows.push([
        "其他",
        "var(--faint)",
        Math.max(0, totals[i] - rows.reduce((a, x) => a + x[2], 0)),
      ]);
    }
    for (const [nm, color, v] of rows.filter((x) => x[2] > 0).sort((a, b) => b[2] - a[2])) {
      const row = document.createElement("div");
      const sw = document.createElement("i");
      sw.style.background = color;
      const label = document.createElement("span");
      label.textContent = nm;
      const val = document.createElement("em");
      val.textContent = ledFormat(ledMetric, v);
      row.append(sw, label, val);
      tip.append(row);
    }
    const sum = document.createElement("div");
    sum.className = "sum";
    const a = document.createElement("span");
    a.textContent = speed ? "本时段" : "总计";
    const b = document.createElement("em");
    b.textContent = ledFormat(ledMetric, totals[i]);
    if (ledMetric !== "calls" && p.calls)
      b.textContent += ` · ${p.calls.toLocaleString()} 次请求`;
    sum.append(a, b);
    tip.append(sum);
    tip.hidden = false;
    const x = M.l + slot * (i + 0.5);
    const w = tip.offsetWidth;
    tip.style.left = Math.max(0, x + 14 + w > W ? x - w - 14 : x + 14) + "px";
  };
  g.onpointermove = at;
  g.onpointerdown = at;
  g.onpointerleave = () => {
    hot.style.display = "none";
    tip.hidden = true;
  };
  box.emphasize = (key) => {
    for (const r of segs) {
      r.style.opacity = key != null && r.dataset.k !== key ? ".22" : "";
    }
  };
  drawLedgerRank(d, bucket);
}

// the ranking beside the chart: who the requests were of, by the metric, the
// most first, each with its share, its figures and a bar in its colour.
function drawLedgerRank(d, bucket) {
  const box = document.getElementById("ledRank");
  if (!box) return;
  const kept = box.scrollTop;
  box.replaceChildren();
  const { top, rest, list } = ledCategories(ledSplit, ledMetric);
  if (!list.length) return;
  const total = list.reduce((a, x) => a + ledValue(ledMetric, x), 0) || 1;
  const leader = Math.max(1, ledRankValue(ledMetric, top[0]));
  const one = (x, color, plain) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "rk" + (plain ? " plain" : "");
    const v = ledRankValue(ledMetric, x);
    const gs = ledGroupSpeed(x);
    const head = document.createElement("div");
    head.className = "rk-a";
    const sw = document.createElement("i");
    sw.className = "rk-sw";
    sw.style.background = color;
    head.append(sw);
    const name = document.createElement("span");
    name.className = "rk-nm";
    name.textContent = ledName(x, ledSplit);
    name.title = name.textContent;
    const val = document.createElement("span");
    val.className = "rk-val";
    val.textContent = ledMetric === "speed" ? (gs ? gs.main : "—") : ledFormat(ledMetric, v);
    if (ledMetric === "speed" && gs) val.title = gs.sub;
    head.append(name, val);
    const bar = document.createElement("div");
    bar.className = "rk-bar";
    const fill = document.createElement("i");
    fill.style.width = Math.max(1.5, (100 * v) / leader).toFixed(1) + "%";
    fill.style.background = color;
    bar.append(fill);
    const more = document.createElement("div");
    more.className = "rk-b";
    const facts = [];
    if (ledMetric !== "calls") facts.push(`${(x.calls || 0).toLocaleString()} 次请求`);
    if (ledMetric !== "tokens") facts.push(`${fmtN(x.tokens || 0)} Token`);
    if (ledMetric !== "cost") facts.push(fmtMoney(x.cost));
    if (x.errors) facts.push(`${x.errors} 次失败`);
    // a speed is no one's share of all the tokens
    if (ledMetric !== "speed") facts.push(`${Math.round((100 * v) / total)}%`);
    more.textContent = facts.join(" · ");
    b.append(head, bar, more);
    if (!plain) {
      b.onpointerenter = () => box.parentElement?.querySelector(".led-chart")?.emphasize?.(x.id);
      b.onpointerleave = () => box.parentElement?.querySelector(".led-chart")?.emphasize?.(null);
    }
    return b;
  };
  for (const x of top) box.append(one(x, x.color));
  if (rest.length) {
    const other = rest.reduce(
      (acc, x) => {
        for (const k of [
          "calls", "tokens", "cost", "errors", "output",
          "floor_out", "floor_ms", "floor_n",
        ])
          acc[k] = (acc[k] || 0) + (x[k] || 0);
        return acc;
      },
      { id: "\0other", name: `其他 (${rest.length})` },
    );
    box.append(one(other, "var(--faint)", true));
  }
  box.scrollTop = kept;
}

// ---- the heatmap: 53 weeks of days, Monday on top ------------------------
function heatDate(s) {
  const [y, m, d] = String(s).split("-").map(Number);
  return new Date(y, m - 1, d);
}

function heatISO(d) {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

// the five levels, from the distribution of the days that had anything: a
// small store reads by its own scale rather than by an absolute one
function heatLevels(values) {
  const nz = values.filter((v) => v > 0).sort((a, b) => a - b);
  if (!nz.length) return () => 0;
  if (nz.length < 8) {
    const most = nz[nz.length - 1];
    return (v) => (v <= 0 ? 0 : Math.min(4, Math.max(1, Math.ceil((4 * v) / most - 1e-9))));
  }
  const q = (p) => nz[Math.min(nz.length - 1, Math.floor(p * nz.length))];
  const [a, b, c, top] = [q(0.25), q(0.5), q(0.75), nz[nz.length - 1]];
  return (v) => (v <= 0 ? 0 : v >= top ? 4 : v <= a ? 1 : v <= b ? 2 : v <= c ? 3 : 4);
}

function drawLedgerHeat() {
  const box = document.getElementById("ledHeat");
  const scroll = document.getElementById("ledHeatScroll");
  if (!box || !scroll || !usageDigest) return;
  const d = usageDigest;
  if (!d.heat_from || !d.heat_to) {
    box.hidden = true;
    return;
  }
  box.hidden = false;
  const at = new Map((d.heat || []).map((x) => [x.date, x]));
  const first = heatDate(d.heat_from);
  const last = heatDate(d.heat_to);
  const days = [];
  for (let cur = new Date(first); cur <= last; cur.setDate(cur.getDate() + 1)) {
    const x = at.get(heatISO(cur)) || { calls: 0, tokens: 0, cost: 0 };
    days.push({ ...x, day: new Date(cur) });
  }
  const value = (x) =>
    ledHeatMetric === "cost" ? x.cost : ledHeatMetric === "calls" ? x.calls : x.tokens;
  const level = heatLevels(days.map(value));
  const shape = [d.heat_from, d.heat_to].join("|");
  let grid = scroll.querySelector(".heat-grid");
  if (!grid || grid.dataset.shape !== shape) {
    grid = document.createElement("div");
    grid.className = "heat-grid";
    grid.dataset.shape = shape;
    grid.setAttribute("role", "img");
    const names = ["一", "二", "三", "四", "五", "六", "日"];
    for (let i = 0; i < 7; i++) {
      const l = document.createElement("span");
      l.className = "wd";
      l.textContent = i % 2 ? "" : names[i];
      l.style.gridArea = `${i + 2} / 1`;
      grid.append(l);
    }
    const weeks = Math.ceil(days.length / 7);
    const labels = [];
    let month = -1;
    days.forEach((x, i) => {
      const w = Math.floor(i / 7);
      const wd = i % 7;
      if (wd === 0 && x.day.getMonth() !== month) {
        month = x.day.getMonth();
        if (
          (!labels.length || w - labels[labels.length - 1].w >= 3) &&
          !(w === 0 && x.day.getDate() > 14) &&
          w < weeks - 1
        ) {
          const m = document.createElement("span");
          m.className = "mo";
          m.textContent = `${x.day.getMonth() + 1}月`;
          labels.push({ m, w });
          grid.append(m);
        }
      }
      const c = document.createElement("i");
      c.dataset.date = heatISO(x.day);
      c.style.gridArea = `${wd + 2} / ${w + 2}`;
      grid.append(c);
    });
    labels.forEach(({ m, w }, i) => {
      m.style.gridArea = `1 / ${w + 2} / 2 / ${(labels[i + 1]?.w ?? weeks) + 2}`;
    });
    grid.onpointerover = (e) => {
      if (e.target.dataset?.date) heatTip(e.target);
    };
    grid.onpointerleave = () => {
      const tip = document.getElementById("ledHeatTip");
      if (tip) tip.hidden = true;
    };
    scroll.replaceChildren(grid);
    // the newest week is what the reader came for: the card opens at its end
    scroll.scrollLeft = scroll.scrollWidth;
  }
  grid.setAttribute("aria-label", "用量热力图");
  const cells = grid.querySelectorAll("i");
  days.forEach((x, i) => {
    const c = cells[i];
    if (!c) return;
    c.className = "l" + level(value(x));
    c.day = x;
  });
  const legend = box.querySelector(".heat-foot");
  if (legend) {
    legend.replaceChildren();
    const span = document.createElement("span");
    span.className = "sess-legend";
    span.append(Object.assign(document.createElement("span"), { textContent: "少" }));
    for (let i = 0; i <= 4; i++) {
      const sw = document.createElement("i");
      sw.className = "l" + i;
      span.append(sw);
    }
    span.append(Object.assign(document.createElement("span"), { textContent: "多" }));
    legend.append(span);
  }
  const tip = document.getElementById("ledHeatTip");
  if (tip && !tip.hidden && tip.cell?.isConnected) heatTip(tip.cell);
  else if (tip) tip.hidden = true;
}

// the tip over a day, above it and kept inside the card
function heatTip(cell) {
  const box = document.getElementById("ledHeat");
  const tip = document.getElementById("ledHeatTip");
  if (!box || !tip || !cell.day) return;
  const x = cell.day;
  tip.replaceChildren();
  const head = document.createElement("b");
  head.textContent = `${x.day.getFullYear()}年${x.day.getMonth() + 1}月${x.day.getDate()}日 周${["日", "一", "二", "三", "四", "五", "六"][x.day.getDay()]}`;
  tip.append(head);
  if (!x.calls) {
    const none = document.createElement("span");
    none.textContent = "没有请求";
    tip.append(none);
  } else {
    const parts = [`${fmtN(x.tokens)} Token`, `${x.calls} 次请求`];
    if (x.cost) parts.push(fmtMoney(x.cost));
    if (x.errors) parts.push(`${x.errors} 次失败`);
    const span = document.createElement("span");
    span.textContent = parts.join(" · ");
    tip.append(span);
  }
  tip.cell = cell;
  tip.hidden = false;
  const b = box.getBoundingClientRect();
  const c = cell.getBoundingClientRect();
  const w = tip.offsetWidth;
  const h = tip.offsetHeight;
  const left = Math.max(6, Math.min(b.width - w - 6, c.left - b.left + c.width / 2 - w / 2));
  let top = c.top - b.top - h - 6;
  if (top < 4) top = c.bottom - b.top + 6;
  tip.style.left = left + "px";
  tip.style.top = top + "px";
}

