<script lang="ts">
  import type { IconName } from '../lib/icons'
  import Icon from './Icon.svelte'

  interface Props {
    icon?: IconName
    title: string
    hint?: string
    action?: string
    tone?: '' | 'ok'
    onaction?: () => void
  }

  let { icon, title, hint, action, tone = '', onaction }: Props = $props()
</script>

<div class="empty-state" class:ok={tone === 'ok'}>
  {#if icon}<span class="glyph"><Icon name={icon} size={26} /></span>{/if}
  <h2>{title}</h2>
  {#if hint}<p>{hint}</p>{/if}
  {#if action}<button class="primary" type="button" onclick={onaction}>{action}</button>{/if}
</div>

<style>
  .glyph {
    color: var(--dim);
  }
  .ok .glyph {
    color: var(--ok);
  }
</style>
