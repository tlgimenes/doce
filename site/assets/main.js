document.addEventListener("DOMContentLoaded", () => {
  initStarCount();
});

const STAR_COUNT_CACHE_KEY = "doce:star-count-cache";
const STAR_COUNT_CACHE_TTL_MS = 60 * 60 * 1000; // ~1 hour, per research.md § 3
const GITHUB_REPO = "tlgimenes/doce";

function initStarCount() {
  const el = document.getElementById("star-count");
  if (!el) return;

  const cached = readStarCountCache();
  if (cached !== null) {
    renderStarCount(el, cached);
    return;
  }

  fetch(`https://api.github.com/repos/${GITHUB_REPO}`)
    .then((response) => {
      if (!response.ok) throw new Error(`GitHub API responded with ${response.status}`);
      return response.json();
    })
    .then((data) => {
      const count = data && typeof data.stargazers_count === "number" ? data.stargazers_count : null;
      if (count === null) throw new Error("Unexpected GitHub API response shape");
      writeStarCountCache(count);
      renderStarCount(el, count);
    })
    .catch(() => {
      // FR-007: any fetch failure keeps the baked-in static fallback already
      // rendered in the page's initial HTML — no error state is shown.
    });
}

function renderStarCount(el, count) {
  el.textContent = count.toLocaleString("en-US");
}

function readStarCountCache() {
  try {
    const raw = localStorage.getItem(STAR_COUNT_CACHE_KEY);
    if (!raw) return null;
    const { count, fetchedAt } = JSON.parse(raw);
    if (typeof count !== "number" || typeof fetchedAt !== "number") return null;
    if (Date.now() - fetchedAt > STAR_COUNT_CACHE_TTL_MS) return null;
    return count;
  } catch {
    return null;
  }
}

function writeStarCountCache(count) {
  try {
    localStorage.setItem(
      STAR_COUNT_CACHE_KEY,
      JSON.stringify({ count, fetchedAt: Date.now() })
    );
  } catch {
    // localStorage unavailable (private browsing, quota, etc.) — safe to skip caching.
  }
}
