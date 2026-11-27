<script lang="ts">
  // Brand moment for sign-in and setup: two grey detours and the direct route in pink,
  // meeting at one terminal behind the card. Decorative only, hidden from assistive technology.
</script>

<svg class="traces" viewBox="0 0 1200 800" preserveAspectRatio="xMidYMid slice" aria-hidden="true" focusable="false">
  <g class="detours">
    <path d="M0 180H360a60 60 0 0 1 60 60V340a60 60 0 0 0 60 60H600" />
    <path d="M0 620H360a60 60 0 0 0 60-60V460a60 60 0 0 1 60-60H600" />
    <path d="M1200 140H900a60 60 0 0 0-60 60V320" />
    <path d="M1200 660H900a60 60 0 0 1-60-60V480" />
  </g>
  <path class="route" d="M0 400H600" />
  <circle class="terminal" cx="600" cy="400" r="6" />
</svg>

<style>
  .traces {
    position: fixed;
    inset: 0;
    width: 100%;
    height: 100%;
    z-index: 0;
    pointer-events: none;
  }
  .detours {
    fill: none;
    stroke: var(--border-subtle);
    stroke-width: 2;
    stroke-linecap: round;
  }
  .route {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2;
    stroke-linecap: round;
    opacity: 0.55;
  }
  .terminal {
    fill: var(--accent);
    opacity: 0.8;
  }
</style>
