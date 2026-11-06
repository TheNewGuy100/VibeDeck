<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import '../library-manager.css';
import type { OrphanReport } from '../interfaces/library';
import { buildSourceCards, formatBytes, isSourceId, normalizeLibraryName, pluralize, validateLibraryName, type LibrarySummary, type SourceId } from '../services/library-manager';

export interface FavoriteSummary { id: string; name: string; count: number }
export interface YoutubeSummary { connected: boolean; playlists: number; tracks: number }
export interface YoutubePlaylistSummary { id: string; name: string; count: number }

type Dialog =
  | { kind: 'create' }
  | { kind: 'rename'; id: string }
  | { kind: 'delete'; id: string }
  | { kind: 'folder'; libraryId: string | null }
  | { kind: 'purge' };

const SOURCE_STORAGE_KEY = 'vibedeck.sources.selected';

const props = defineProps<{
  libraries: LibrarySummary[];
  favorites: FavoriteSummary[];
  youtube: YoutubeSummary;
  youtubePlaylists: YoutubePlaylistSummary[];
  orphans: OrphanReport | null;
  busy: boolean;
  /** Shows the sources screen; the dialogs stay mounted so other screens can open them. */
  hub: boolean;
}>();
const emit = defineEmits<{
  open: [id: string];
  create: [name: string];
  rename: [id: string, name: string];
  delete: [id: string];
  importFiles: [libraryId: string | null];
  importFolder: [libraryId: string | null, subfoldersAsLibraries: boolean];
  purge: [];
  connect: [];
  disconnect: [];
}>();

function readStoredSource(): SourceId {
  try {
    const stored = localStorage.getItem(SOURCE_STORAGE_KEY);
    return isSourceId(stored) ? stored : 'local';
  } catch {
    return 'local';
  }
}

const source = ref<SourceId>(readStoredSource());
const dialog = ref<Dialog | null>(null);
const name = ref('');
const subfolders = ref(false);
const nameInput = ref<HTMLInputElement | null>(null);
const panel = ref<HTMLElement | null>(null);
const tabButtons = ref<HTMLButtonElement[]>([]);
let returnFocus: HTMLElement | null = null;

const cards = computed(() => buildSourceCards({
  libraries: props.libraries,
  favoriteTracks: props.favorites.find((favorite) => favorite.id === 'favorites')?.count ?? 0,
  youtube: props.youtube,
}));
const target = computed(() => {
  const current = dialog.value;
  return current && 'id' in current ? props.libraries.find((library) => library.id === current.id) ?? null : null;
});
const nameError = computed(() => {
  const current = dialog.value;
  if (!current || (current.kind !== 'create' && current.kind !== 'rename')) return null;
  if (!name.value) return null;
  return validateLibraryName(name.value, props.libraries, current.kind === 'rename' ? current.id : undefined);
});
const canSubmitName = computed(() => name.value.trim() !== '' && !nameError.value);
const folderTarget = computed(() => {
  const current = dialog.value;
  return current?.kind === 'folder' ? props.libraries.find((library) => library.id === current.libraryId) ?? null : null;
});
const totalBytes = computed(() => props.libraries.reduce((total, library) => total + library.bytes, 0));

function selectSource(id: SourceId) {
  source.value = id;
  try { localStorage.setItem(SOURCE_STORAGE_KEY, id); } catch { /* the choice just is not remembered */ }
}

/** Arrow keys move between the source tabs, as in a tab list. */
function onTabKeydown(event: KeyboardEvent, index: number) {
  const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
  if (!step && event.key !== 'Home' && event.key !== 'End') return;
  event.preventDefault();
  const last = cards.value.length - 1;
  const next = event.key === 'Home' ? 0 : event.key === 'End' ? last : (index + step + cards.value.length) % cards.value.length;
  selectSource(cards.value[next].id);
  void nextTick(() => tabButtons.value[next]?.focus());
}

function show(next: Dialog, initialName = '') {
  returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  name.value = initialName;
  subfolders.value = false;
  dialog.value = next;
}
function close() { dialog.value = null; }

function openCreate() { show({ kind: 'create' }); }
function openRename(id: string) {
  const library = props.libraries.find((item) => item.id === id);
  if (library) show({ kind: 'rename', id }, library.name);
}
function openDelete(id: string) { if (props.libraries.some((item) => item.id === id)) show({ kind: 'delete', id }); }
function openFolderImport(libraryId: string | null = null) { show({ kind: 'folder', libraryId }); }
function openPurge() { if (props.orphans?.count) show({ kind: 'purge' }); }

function submit() {
  const current = dialog.value;
  if (!current) return;
  if (current.kind === 'create' && canSubmitName.value) emit('create', normalizeLibraryName(name.value));
  else if (current.kind === 'rename' && canSubmitName.value) emit('rename', current.id, normalizeLibraryName(name.value));
  else if (current.kind === 'delete') emit('delete', current.id);
  else if (current.kind === 'folder') emit('importFolder', current.libraryId, subfolders.value);
  else if (current.kind === 'purge') emit('purge');
  else return;
  close();
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape' && dialog.value) { event.preventDefault(); event.stopPropagation(); close(); }
}

watch(dialog, (value) => {
  if (value) {
    document.addEventListener('keydown', onKeydown, true);
    void nextTick(() => (nameInput.value ?? panel.value?.querySelector<HTMLElement>('button.primary'))?.focus());
  } else {
    document.removeEventListener('keydown', onKeydown, true);
    returnFocus?.focus();
    returnFocus = null;
  }
});
onBeforeUnmount(() => document.removeEventListener('keydown', onKeydown, true));

defineExpose({ openCreate, openRename, openDelete, openFolderImport, openPurge });
</script>

<template>
  <div class="lib-root">
    <div v-show="hub" class="lib-hub">
      <div class="lib-sources" role="tablist" aria-label="Fontes">
        <button
          v-for="(card, index) in cards"
          :id="`lib-tab-${card.id}`"
          :key="card.id"
          :ref="(element) => { if (element) tabButtons[index] = element as HTMLButtonElement; }"
          type="button"
          role="tab"
          :aria-selected="source === card.id"
          :aria-controls="`lib-panel-${card.id}`"
          :tabindex="source === card.id ? 0 : -1"
          :class="['lib-source', { active: source === card.id }]"
          :style="{ '--accent': card.accent }"
          @click="selectSource(card.id)"
          @keydown="onTabKeydown($event, index)"
        >
          <span class="lib-source-dot" aria-hidden="true"></span>
          <strong>{{ card.name }}</strong>
          <span class="lib-source-count">{{ card.countLabel }}</span>
          <small>{{ card.description }}</small>
        </button>
      </div>

      <section v-if="source === 'local'" id="lib-panel-local" class="lib-section" role="tabpanel" aria-labelledby="lib-tab-local">
        <div class="lib-heading">
          <div>
            <p class="eyebrow">BIBLIOTECAS LOCAIS</p>
            <h2>{{ pluralize(libraries.length, 'biblioteca', 'bibliotecas') }}<small v-if="libraries.length"> · {{ formatBytes(totalBytes) }}</small></h2>
          </div>
          <div class="lib-actions">
            <button type="button" class="lib-btn" :disabled="busy" @click="emit('importFiles', null)">Importar arquivos</button>
            <button type="button" class="lib-btn" :disabled="busy" @click="openFolderImport(null)">Importar pasta</button>
            <button type="button" class="lib-btn primary" @click="openCreate">+ Nova biblioteca</button>
          </div>
        </div>

        <div v-if="orphans?.count" class="lib-orphans" role="status">
          <span><strong>{{ pluralize(orphans.count, 'arquivo', 'arquivos') }} sem uso</strong> · {{ formatBytes(orphans.bytes) }} podem ser liberados. Nenhuma biblioteca nem favorito usa mais esses arquivos.</span>
          <button type="button" class="lib-btn" @click="openPurge">Revisar e limpar</button>
        </div>

        <div v-if="libraries.length" class="lib-grid">
          <article v-for="library in libraries" :key="library.id" class="lib-card">
            <button type="button" class="lib-card-main" @click="emit('open', library.id)">
              <strong>{{ library.name }}</strong>
              <span>{{ pluralize(library.count, 'faixa', 'faixas') }} · {{ formatBytes(library.bytes) }}</span>
              <small v-if="library.unavailable" class="lib-warn">{{ pluralize(library.unavailable, 'arquivo ausente', 'arquivos ausentes') }}</small>
            </button>
            <div class="lib-card-actions">
              <button type="button" class="lib-link" :disabled="busy" @click="emit('importFiles', library.id)">Importar aqui</button>
              <button type="button" class="lib-link" @click="openRename(library.id)">Renomear</button>
              <button type="button" class="lib-link danger" @click="openDelete(library.id)">Excluir</button>
            </div>
          </article>
        </div>
        <div v-else class="lib-empty">
          <strong>Nenhuma biblioteca local ainda</strong>
          <span>Importe arquivos ou uma pasta (cada pasta vira uma biblioteca), ou crie uma biblioteca vazia.</span>
        </div>
      </section>

      <section v-else-if="source === 'youtube'" id="lib-panel-youtube" class="lib-section" role="tabpanel" aria-labelledby="lib-tab-youtube">
        <div class="lib-heading">
          <div>
            <p class="eyebrow">YOUTUBE MUSIC</p>
            <h2>{{ youtube.connected ? pluralize(youtube.playlists, 'playlist sincronizada', 'playlists sincronizadas') : 'Conta não conectada' }}</h2>
          </div>
          <div class="lib-actions">
            <button v-if="youtube.connected" type="button" class="lib-btn" @click="emit('disconnect')">Desconectar</button>
            <button v-else type="button" class="lib-btn primary" @click="emit('connect')">Conectar conta</button>
          </div>
        </div>
        <template v-if="youtube.connected">
          <div v-if="youtubePlaylists.length" class="lib-grid">
            <button v-for="playlist in youtubePlaylists" :key="playlist.id" type="button" class="lib-card lib-card-button" @click="emit('open', playlist.id)">
              <strong>{{ playlist.name }}</strong>
              <span>{{ playlist.count ? pluralize(playlist.count, 'faixa', 'faixas') : 'Abra para carregar as faixas' }}</span>
            </button>
          </div>
          <div v-else class="lib-empty"><strong>Nenhuma playlist sincronizada</strong><span>As playlists da sua conta aparecem aqui depois da sincronização.</span></div>
        </template>
        <div v-else class="lib-empty">
          <strong>Conecte sua conta para sincronizar</strong>
          <span>Suas playlists do YouTube Music aparecem aqui. Seus favoritos do YouTube continuam listados, mas só tocam com a conta conectada.</span>
        </div>
      </section>

      <section v-else id="lib-panel-favorites" class="lib-section" role="tabpanel" aria-labelledby="lib-tab-favorites">
        <div class="lib-heading"><div><p class="eyebrow">FAVORITOS</p><h2>Em todas as fontes</h2></div></div>
        <div class="lib-grid">
          <button v-for="favorite in favorites" :key="favorite.id" type="button" class="lib-card lib-card-button" @click="emit('open', favorite.id)">
            <strong>{{ favorite.name }}</strong>
            <span>{{ pluralize(favorite.count, 'faixa', 'faixas') }}</span>
          </button>
        </div>
      </section>
    </div>

    <div v-if="dialog" class="lib-overlay" @click.self="close">
      <form ref="panel" class="lib-dialog" role="dialog" aria-modal="true" :aria-label="dialog.kind === 'create' ? 'Nova biblioteca' : dialog.kind === 'rename' ? 'Renomear biblioteca' : dialog.kind === 'delete' ? 'Excluir biblioteca' : dialog.kind === 'folder' ? 'Importar pasta' : 'Limpar arquivos sem uso'" @submit.prevent="submit">
        <template v-if="dialog.kind === 'create' || dialog.kind === 'rename'">
          <h3>{{ dialog.kind === 'create' ? 'Nova biblioteca' : 'Renomear biblioteca' }}</h3>
          <label class="lib-field">Nome
            <input ref="nameInput" v-model="name" type="text" maxlength="120" autocomplete="off" :aria-invalid="Boolean(nameError)" aria-describedby="lib-name-error" />
          </label>
          <p id="lib-name-error" class="lib-error" role="alert">{{ nameError }}</p>
        </template>

        <template v-else-if="dialog.kind === 'delete' && target">
          <h3>Excluir “{{ target.name }}”?</h3>
          <p>A biblioteca e a lista de {{ pluralize(target.count, 'faixa', 'faixas') }} serão removidas. Os arquivos continuam no app até você revisar a limpeza de arquivos sem uso, e favoritos não são afetados.</p>
        </template>

        <template v-else-if="dialog.kind === 'folder'">
          <h3>Importar pasta</h3>
          <p>Os arquivos de áudio serão copiados para o app. Cada pasta escolhida vira uma biblioteca com o nome dela.</p>
          <label class="lib-check"><input v-model="subfolders" type="checkbox" /> Cada subpasta vira uma biblioteca própria</label>
          <p class="lib-note">Arquivos soltos vão para {{ folderTarget ? `“${folderTarget.name}”` : '“Biblioteca local”' }}. Conteúdo repetido não é copiado de novo, só referenciado.</p>
        </template>

        <template v-else-if="dialog.kind === 'purge' && orphans">
          <h3>Limpar arquivos sem uso?</h3>
          <p>Serão apagados {{ pluralize(orphans.count, 'arquivo', 'arquivos') }} do disco, liberando {{ formatBytes(orphans.bytes) }}. Isso não pode ser desfeito; para ouvir de novo será preciso importar outra vez.</p>
        </template>

        <div class="lib-dialog-actions">
          <button type="button" class="lib-btn" @click="close">Cancelar</button>
          <button type="submit" :class="['lib-btn', 'primary', { danger: dialog.kind === 'delete' || dialog.kind === 'purge' }]" :disabled="(dialog.kind === 'create' || dialog.kind === 'rename') && !canSubmitName">
            {{ dialog.kind === 'create' ? 'Criar' : dialog.kind === 'rename' ? 'Salvar' : dialog.kind === 'delete' ? 'Excluir biblioteca' : dialog.kind === 'folder' ? 'Escolher pasta…' : 'Apagar arquivos' }}
          </button>
        </div>
      </form>
    </div>
  </div>
</template>
