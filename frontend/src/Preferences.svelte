<script lang="ts">
  import Icon from './Icon.svelte';
  import type { Language, Messages, Theme } from './i18n';

  let { t, language = $bindable(), theme = $bindable() }: { t: Messages; language: Language; theme: Theme } = $props();
</script>

<div class="preferences" role="group" aria-label={t.preferences}>
  <label class="preference">
    <Icon name="globe" />
    <span class="visually-hidden">{t.language}</span>
    <select bind:value={language}>
      <option value="en" lang="en">English</option>
      <option value="de" lang="de">Deutsch</option>
    </select>
  </label>
  <label class="preference">
    <Icon name="theme" />
    <span class="visually-hidden">{t.theme}</span>
    <select bind:value={theme}>
      <option value="dark">{t.dark}</option>
      <option value="light">{t.light}</option>
      <option value="system">{t.system}</option>
    </select>
  </label>
</div>
