(function () {
  try {
    var t = localStorage.getItem("agenthub_theme") || "system";
    var d =
      t === "dark" ||
      (t === "system" &&
        window.matchMedia("(prefers-color-scheme: dark)").matches);
    document.documentElement.setAttribute("data-theme", d ? "dark" : "light");
    document.documentElement.style.colorScheme = d ? "dark" : "light";
    var m = document.querySelectorAll('meta[name="theme-color"]');
    for (var i = 0; i < m.length; i++) {
      m[i].setAttribute("content", d ? "#0f1110" : "#f7faf8");
    }
  } catch (_e) {}
})();
