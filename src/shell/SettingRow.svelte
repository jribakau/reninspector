<script lang="ts">
  import Icon from '../components/Icon.svelte'
  import { isModified, resetSetting, setSetting, settings, type SettingDef, type SettingKey } from '../lib/settings.svelte'

  let { def }: { def: SettingDef } = $props()

  const value = $derived(settings[def.key])
  const changed = $derived(isModified(def.key))
  const off = $derived(def.key === 'autosaveDelay' && settings.autosave !== 'delay')

  function commit(key: SettingKey, next: unknown) {
    setSetting(key, next as never)
  }

  function onNumber(e: Event & { currentTarget: HTMLInputElement }) {
    const input = e.currentTarget
    if (input.value.trim() !== '' && Number.isFinite(input.valueAsNumber)) commit(def.key, input.valueAsNumber)
    input.value = String(settings[def.key])
  }

  function onEnum(e: Event & { currentTarget: HTMLSelectElement }) {
    const option = def.options?.find((o) => String(o.value) === e.currentTarget.value)
    if (option) commit(def.key, option.value)
  }

  function onText(e: Event & { currentTarget: HTMLInputElement }) {
    commit(def.key, e.currentTarget.value)
    e.currentTarget.value = String(settings[def.key])
  }
</script>

<div class="setting-row" class:changed class:off>
  <div class="text">
    <label class="setting-name" for={`set-${def.key}`}>{def.label}</label>
    <p class="setting-desc">{def.description}</p>
  </div>
  <div class="ctl">
    {#if def.type === 'bool'}
      <input
        id={`set-${def.key}`}
        type="checkbox"
        checked={value as boolean}
        onchange={(e) => commit(def.key, e.currentTarget.checked)}
      />
    {:else if def.type === 'number'}
      <input
        id={`set-${def.key}`}
        class="num"
        type="number"
        min={def.min}
        max={def.max}
        step={def.step}
        value={value as number}
        disabled={off}
        onchange={onNumber}
      />
      {#if def.unit}<span class="unit">{def.unit}</span>{/if}
    {:else if def.type === 'enum'}
      <select id={`set-${def.key}`} onchange={onEnum}>
        {#each def.options ?? [] as option (option.value)}
          <option value={String(option.value)} selected={option.value === value}>{option.label}</option>
        {/each}
      </select>
    {:else}
      <input
        id={`set-${def.key}`}
        type="text"
        class="wide"
        placeholder={def.placeholder}
        value={value as string}
        onchange={onText}
      />
    {/if}
    <button
      type="button"
      class="icon sm ghost reset"
      aria-label={`Reset ${def.label}`}
      title="Reset to default"
      disabled={!changed}
      onclick={() => resetSetting(def.key)}
    >
      <Icon name="undo" size={12} />
    </button>
  </div>
</div>

<style>
  .ctl {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    justify-content: flex-end;
  }
  .ctl select { min-width: 160px; }
  .num { width: 88px; }
  .wide { width: 240px; }
  .unit { color: var(--dim); font-size: var(--fs-md); }
  .reset { flex: none; }
</style>
