// Space switcher. A person can belong to several spaces (tenants), each on its
// own subdomain and often under a different email, so there is no single
// server-side identity that spans them. Instead each space's session is a
// host-only cookie, and this script keeps a small, non-sensitive directory of
// the spaces this browser has signed into in a domain-wide `lb_spaces` cookie.
//
// On every page it renders that directory as a dropdown in the top bar (showing
// the current login) and refreshes it from /whoami for the current space.
(function () {
  "use strict";
  var COOKIE = "lb_spaces";
  var YEAR = 60 * 60 * 24 * 365;

  function esc(s) {
    return String(s == null ? "" : s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#x27;" }[c];
    });
  }

  function readSpaces() {
    var m = document.cookie.match(/(?:^|;\s*)lb_spaces=([^;]+)/);
    if (!m) return [];
    try {
      var v = JSON.parse(decodeURIComponent(m[1]));
      return Array.isArray(v) ? v : [];
    } catch (e) {
      return [];
    }
  }

  function writeSpaces(list, cookieDomain) {
    var attrs = "; path=/; max-age=" + YEAR + "; samesite=lax";
    if (location.protocol === "https:") attrs += "; secure";
    if (cookieDomain) attrs += "; domain=" + cookieDomain;
    document.cookie = COOKIE + "=" + encodeURIComponent(JSON.stringify(list)) + attrs;
  }

  function currentSlug() {
    return location.hostname.split(".")[0];
  }

  function spaceUrl(slug, cookieDomain) {
    if (cookieDomain) return location.protocol + "//" + slug + "." + cookieDomain + "/today";
    // Single-host setup (e.g. local dev): only the current space is reachable.
    return slug === currentSlug() ? "/today" : null;
  }

  function render(list, cookieDomain, currentEmail) {
    var mount = document.getElementById("space-switcher");
    if (!mount) return;
    if (!list.length) {
      mount.innerHTML = "";
      return;
    }
    var cur = currentSlug();
    var here = null;
    for (var i = 0; i < list.length; i++) {
      if (list[i].slug === cur) here = list[i];
    }
    var label = here
      ? esc(here.name) + " · " + esc(currentEmail || here.email)
      : "Spaces";

    var items = list
      .map(function (s) {
        var isCur = s.slug === cur;
        var url = spaceUrl(s.slug, cookieDomain);
        var inner =
          '<span class="ss-name">' + esc(s.name) + (isCur ? " ✓" : "") + "</span>" +
          '<span class="ss-email">' + esc(s.email) + "</span>";
        if (url && !isCur) {
          return '<li><a href="' + esc(url) + '">' + inner + "</a></li>";
        }
        return '<li class="' + (isCur ? "cur" : "disabled") + '">' + inner + "</li>";
      })
      .join("");

    mount.innerHTML =
      '<details class="space-dd">' +
      "<summary>" + label + "</summary>" +
      '<ul class="ss-list">' + items + "</ul>" +
      "</details>";
  }

  if (!document.getElementById("space-switcher")) return;

  // Render immediately from the existing directory, then refresh from the
  // server for the current space (adds/updates this space's entry).
  render(readSpaces(), null, null);

  fetch("/whoami", { credentials: "same-origin" })
    .then(function (r) {
      return r.status === 200 ? r.json() : null;
    })
    .then(function (me) {
      if (!me || !me.slug) return;
      var list = readSpaces().filter(function (s) {
        return s.slug !== me.slug;
      });
      list.push({ slug: me.slug, name: me.name, email: me.email });
      list.sort(function (a, b) {
        return String(a.name || "").localeCompare(String(b.name || ""));
      });
      writeSpaces(list, me.cookie_domain || null);
      render(list, me.cookie_domain || null, me.email);
    })
    .catch(function () {});
})();
