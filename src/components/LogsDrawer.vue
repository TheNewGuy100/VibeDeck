<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import '../logs-drawer.css';
import type { AppLogEntry, AppWebviewInfo, LogLevel } from '../services/app-logs';
import { countLogs, filterLogs, formatLogDetails, formatLogTime, isNearBottom, logEntryToText, sourceHue } from '../services/log-view';

const props = defineProps<{
  open: boolean;
  logs: AppLogEntry[];
  webviews: AppWebviewInfo[];
  changingWebviewLabel: string | null;
}>();
const emit = defineEmits<{
  'update:open': [value: boolean];
  export: [];
  refreshWebviews: [];
  toggleWebview: [webview: AppWebviewInfo];
  copied: [count: number];
}>();

const tab = ref<'events' | 'webviews'>('events');
const search = ref('');
const minLevel = ref<LogLevel>('debug');
const sources = ref(new Set<string>());
const expandedId = ref<number | null>(null);
const following = ref(true);
const unseen = ref(0);
const list = ref<HTMLElement | null>(null);
const dialog = ref<HTMLElement | null>(null);
let returnFocus: HTMLElement | null = null;

const counts = computed(() => countLogs(props.logs));
const visible = computed(() => filterLogs(props.logs, { minLevel: minLevel.value, sources: sources.value, search: search.value }));
const filtering = computed(() => minLevel.value !== 'debug' || sources.value.size > 0 || search.value.trim() !== '');
const levelOptions = computed<{ id: LogLevel; label: string; count: number }[]>(() => {
  const { debug, info, warn, error } = counts.value.byLevel;
  return [
    { id: 'debug', label: 'Tudo', count: debug + info + warn + error },
    { id: 'info', label: 'Info+', count: info + warn + error },
    { id: 'warn', label: 'Avisos+', count: warn + error },
    { id: 'error', label: 'Erros', count: error },
  ];
});

function close() { emit('update:open', false); }
function toggleSource(source: string) {
  const next = new Set(sources.value);
  if (next.has(source)) next.delete(source); else next.add(source);
  sources.value = next;
}
function resetFilters() { search.value = ''; minLevel.value = 'debug'; sources.value = new Set(); }
function toggleExpanded(id: number) { expandedId.value = expandedId.value === id ? null : id; }

function scrollToEnd() {
  const element = list.value;
  if (!element) return;
  element.scrollTop = element.scrollHeight;
  following.value = true;
  unseen.value = 0;
}
function onScroll() {
  const element = list.value;
  if (!element) return;
  following.value = isNearBottom(element.scrollTop, element.clientHeight, element.scrollHeight);
  if (following.value) unseen.value = 0;
}

async function copyEntries(entries: AppLogEntry[]) {
  try {
    await navigator.clipboard.writeText(entries.map(logEntryToText).join('\n\n'));
    emit('copied', entries.length);
  } catch {
    // Clipboard access can be denied; the export button remains the fallback.
  }
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') { event.preventDefault(); close(); }
}

watch(() => visible.value.length, (length, previous) => {
  if (tab.value !== 'events') return;
  if (following.value) void nextTick(scrollToEnd);
  else if (length > previous) unseen.value += length - previous;
});
watch([search, minLevel, sources], () => { void nextTick(scrollToEnd); });
watch(tab, (value) => { if (value === 'events') void nextTick(scrollToEnd); else emit('refreshWebviews'); });
watch(() => props.open, (value) => {
  if (value) {
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    document.addEventListener('keydown', onKeydown);
    following.value = true;
    void nextTick(() => { scrollToEnd(); dialog.value?.focus(); });
  } else {
    document.removeEventListener('keydown', onKeydown);
    returnFocus?.focus();
    returnFocus = null;
  }
}, { immediate: true });
onBeforeUnmount(() => document.removeEventListener('keydown', onKeydown));
</script>

<template>
  <Transition name="logs-drawer">
    <div v-if="open" ref="dialog" class="logs-drawer" role="dialog" aria-modal="true" aria-label="Logs do VibeDeck" tabindex="-1">
      <header class="logs-drawer-head">
        <div class="logs-drawer-title">
          <p class="eyebrow">DIAGNÓSTICO</p>
          <h2>Logs</h2>
        </div>
        <span :class="['logs-live', { paused: !following }]" role="status">
          <span class="logs-live-dot" aria-hidden="true"></span>{{ following ? 'Ao vivo' : 'Pausado na leitura' }}
        </span>
        <div class="logs-drawer-stats">
          <span><strong>{{ counts.total }}</strong> eventos</span>
          <span :class="{ bad: counts.error > 0 }"><strong>{{ counts.error }}</strong> erros</span>
          <span :class="{ warn: counts.byLevel.warn > 0 }"><strong>{{ counts.byLevel.warn }}</strong> avisos</span>
          <span v-if="filtering"><strong>{{ visible.length }}</strong> exibidos</span>
        </div>
        <div class="logs-drawer-actions">
          <button type="button" class="logs-btn" :disabled="!visible.length" @click="copyEntries(visible)">Copiar{{ filtering ? ' filtrados' : '' }}</button>
          <button type="button" class="logs-btn" @click="emit('export')">Exportar</button>
          <button type="button" class="logs-btn primary" aria-label="Fechar logs (Esc)" @click="close">Fechar <kbd>Esc</kbd></button>
        </div>
      </header>

      <div class="logs-tabs" role="tablist" aria-label="Seções de diagnóstico">
        <button type="button" role="tab" :aria-selected="tab === 'events'" :class="{ active: tab === 'events' }" @click="tab = 'events'">Eventos</button>
        <button type="button" role="tab" :aria-selected="tab === 'webviews'" :class="{ active: tab === 'webviews' }" @click="tab = 'webviews'">Janelas <span class="logs-tab-count">{{ webviews.length }}</span></button>
      </div>

      <template v-if="tab === 'events'">
        <div class="logs-filters">
          <label class="logs-search"><span aria-hidden="true">⌕</span>
            <input v-model="search" type="search" placeholder="Buscar em mensagem, fonte, operação, código…" aria-label="Buscar nos logs" />
          </label>
          <div class="logs-segmented" role="group" aria-label="Nível">
            <button v-for="option in levelOptions" :key="option.id" type="button" :class="[`lv-${option.id}`, { active: minLevel === option.id }]" :aria-pressed="minLevel === option.id" :title="option.id === 'debug' ? 'Mostrar todos os níveis' : `Mostrar ${option.label.replace('+', ' e acima')}`" @click="minLevel = option.id">
              {{ option.label }} <span>{{ option.count }}</span>
            </button>
          </div>
          <button v-if="filtering" type="button" class="logs-btn ghost" @click="resetFilters">Limpar filtros</button>
        </div>
        <div v-if="counts.bySource.length" class="logs-sources" role="group" aria-label="Fontes">
          <button v-for="item in counts.bySource" :key="item.source" type="button" :style="{ '--h': sourceHue(item.source) }" :class="['logs-chip', { active: sources.has(item.source) }]" :aria-pressed="sources.has(item.source)" @click="toggleSource(item.source)">
            {{ item.source }} <span>{{ item.count }}</span><i v-if="item.errors" :title="`${item.errors} erros`">{{ item.errors }}</i>
          </button>
        </div>

        <div class="logs-stream-wrap">
          <div ref="list" class="logs-stream" role="log" aria-live="off" aria-label="Fluxo de eventos" tabindex="0" @scroll.passive="onScroll">
            <div v-for="entry in visible" :key="entry.id" :class="['logs-row', entry.level, { expanded: expandedId === entry.id }]">
              <button type="button" class="logs-row-main" :aria-expanded="expandedId === entry.id" @click="toggleExpanded(entry.id)">
                <time>{{ formatLogTime(entry.timestamp) }}</time>
                <span class="logs-row-level">{{ entry.level }}</span>
                <span class="logs-row-source" :style="{ '--h': sourceHue(entry.source) }">{{ entry.source }}</span>
                <span class="logs-row-message">{{ entry.message }}</span>
                <span v-if="entry.code" class="logs-row-code">{{ entry.code }}</span>
              </button>
              <div v-if="expandedId === entry.id" class="logs-row-detail">
                <dl>
                  <div><dt>Nível</dt><dd>{{ entry.level }}</dd></div>
                  <div><dt>Operação</dt><dd>{{ entry.operation }}</dd></div>
                  <div><dt>Janela</dt><dd>{{ entry.window ?? 'main/native' }}</dd></div>
                  <div v-if="entry.code"><dt>Código</dt><dd>{{ entry.code }}</dd></div>
                  <div v-if="entry.correlationId"><dt>Correlação</dt><dd>{{ entry.correlationId }}</dd></div>
                  <div><dt>Id</dt><dd>#{{ entry.id }}</dd></div>
                </dl>
                <p class="logs-row-full">{{ entry.message }}</p>
                <pre v-if="formatLogDetails(entry)">{{ formatLogDetails(entry) }}</pre>
                <button type="button" class="logs-btn" @click="copyEntries([entry])">Copiar evento</button>
              </div>
            </div>
            <div v-if="!visible.length" class="logs-empty">
              <strong>{{ filtering ? 'Nenhum evento para estes filtros' : 'Aguardando eventos' }}</strong>
              <span>{{ filtering ? 'Ajuste a busca ou limpe os filtros.' : 'Execute uma ação no app e os eventos chegam aqui em tempo real.' }}</span>
            </div>
          </div>
          <button v-if="!following" type="button" class="logs-jump" @click="scrollToEnd">
            ↓ {{ unseen > 0 ? `${unseen} ${unseen === 1 ? 'novo' : 'novos'}` : 'Ir para o fim' }}
          </button>
        </div>
      </template>

      <section v-else class="logs-webviews" aria-label="Janelas e brokers ativos">
        <div class="logs-webviews-head">
          <p>Janelas e brokers ativos. Atualiza ao abrir esta aba.</p>
          <button type="button" class="logs-btn" @click="emit('refreshWebviews')">Atualizar</button>
        </div>
        <article v-for="webview in webviews" :key="webview.label" class="logs-webview-row">
          <span :class="['logs-live-dot', { off: !webview.visible }]" aria-hidden="true"></span>
          <span class="logs-webview-name"><strong>{{ webview.name }}</strong><small>{{ webview.label }}</small></span>
          <span class="logs-tag">{{ webview.internal ? 'Interna' : 'Aplicativo' }}</span>
          <span :class="['logs-tag', { on: webview.visible }]">{{ webview.visible ? 'Visível' : 'Invisível' }}</span>
          <button v-if="webview.internal" type="button" class="logs-btn" :disabled="changingWebviewLabel === webview.label" @click="emit('toggleWebview', webview)">
            {{ changingWebviewLabel === webview.label ? 'Alterando…' : webview.visible ? 'Ocultar' : 'Mostrar' }}
          </button>
        </article>
        <div v-if="!webviews.length" class="logs-empty"><strong>Nenhuma WebView ativa</strong><span>As janelas aparecem aqui quando forem criadas.</span></div>
      </section>
    </div>
  </Transition>
</template>
