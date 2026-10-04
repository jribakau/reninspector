<script lang="ts">
  import { ICONS, type IconName } from '../lib/icons'

  interface Props {
    name: IconName
    size?: number
    label?: string
  }

  let { name, size = 16, label }: Props = $props()
  const def = $derived(ICONS[name])
</script>

<svg
  class="icon-svg"
  class:solid={def.fill}
  viewBox="0 0 16 16"
  width={size}
  height={size}
  role={label ? 'img' : undefined}
  aria-label={label}
  aria-hidden={label ? undefined : 'true'}
>
  <path d={def.d} />
</svg>

<style>
  .icon-svg {
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .icon-svg.solid {
    fill: currentColor;
    stroke: none;
  }
</style>
