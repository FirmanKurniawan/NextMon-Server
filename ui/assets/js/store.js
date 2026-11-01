// Shared state: status polling, connection state, job tracking, edition helpers.
import { get, sse, on, ApiError } from './api.js';

export const S = { status: null, offline: false, loaded: false };
const subs = new Set();
export const subscribe = (fn) => { subs.add(fn); return () => subs.delete(fn); };
const emit = () => subs.forEach((f) => { try { f(S); } catch (e) { console.error(e); } });

export const EDITIONS = ['php', 'next'];
export const kindOf = (ed) => (ed === 'php' ? 'php' : 'node');
export const editionOf = (kind) => (kind === 'php' ? 'php' : 'next');
export const cfg = () => S.status?.config || {};
export const edition = () => S.status?.edition || cfg().edition || 'php';

let fails = 0, timer = null, inflight = null;
export function refreshStatus() {
  if (inflight) return inflight;
  inflight = get('/status').then((st) => {
    fails = 0; S.status = st; S.loaded = true;
    if (S.offline) { S.offline = false; }
    emit();
    return st;
  }).catch((e) => {
    if (e instanceof ApiError && e.status === 0) { if (++fails >= 2 && !S.offline) { S.offline = true; emit(); } }
    else if (e instanceof ApiError && e.status === 401) { /* handled by auth listener */ }
    throw e;
  }).finally(() => { inflight = null; });
  return inflight;
}
export function startPolling() {
  const tick = async () => {
    if (!document.hidden) { try { await refreshStatus(); } catch { /* offline */ } }
    timer = setTimeout(tick, S.offline ? 4000 : 2500);
  };
  clearTimeout(timer); tick();
  document.addEventListener('visibilitychange', () => { if (!document.hidden) refreshStatus().catch(() => {}); });
  on('net', (bad) => { if (!bad && S.offline) refreshStatus().catch(() => {}); });
}

/* ---- jobs ---- */
const TERMINAL_OK = ['done', 'success', 'succeeded', 'completed'];
const TERMINAL_ERR = ['error', 'failed', 'failure'];
export const jobDone = (j) => TERMINAL_OK.includes(j?.state);
export const jobFailed = (j) => TERMINAL_ERR.includes(j?.state);

/** Follow a job to completion. onUpdate(job) on every change. Resolves with final job, rejects with Error(message) on failure. */
export function followJob(jobId, onUpdate) {
  return new Promise((resolve, reject) => {
    let finished = false, poll = null, stream = null;
    const finish = (job) => {
      if (finished) return; finished = true; stream?.close(); clearInterval(poll);
      if (jobFailed(job)) {
        const err = new Error(job.message || job.error?.message || (typeof job.error === 'string' ? job.error : '') || 'Job failed');
        err.code = job.error?.code || job.code || 'job_failed'; err.job = job; reject(err);
      } else resolve(job);
    };
    const handle = (job) => { if (!job || finished) return; onUpdate?.(job); if (jobDone(job) || jobFailed(job)) finish(job); };
    stream = sse(`/jobs/${encodeURIComponent(jobId)}/stream`, (d) => { try { handle(JSON.parse(d)); } catch { /* keepalive */ } }, { reconnect: false });
    // Safety net: poll as well, so a broken stream never leaves a job hanging.
    poll = setInterval(async () => { try { handle(await get(`/jobs/${encodeURIComponent(jobId)}`)); } catch (e) { if (e.status === 404) finish({ state: 'error', message: 'Job not found' }); } }, 1500);
  });
}

/** Tracked jobs survive page navigation (keyed, e.g. "php:8.4.27"). */
export const jobs = new Map();
const jobSubs = new Set();
export const subscribeJobs = (fn) => { jobSubs.add(fn); return () => jobSubs.delete(fn); };
const emitJobs = () => jobSubs.forEach((f) => f());
export function trackJob(key, jobId, meta = {}) {
  const rec = { key, jobId, progress: 0, step: '', state: 'queued', ...meta };
  jobs.set(key, rec); emitJobs();
  return followJob(jobId, (j) => { Object.assign(rec, { progress: j.progress ?? rec.progress, step: j.step || rec.step, state: j.state, message: j.message }); emitJobs(); })
    .then((j) => { jobs.delete(key); emitJobs(); return j; })
    .catch((e) => { rec.state = 'error'; rec.error = e.message; emitJobs(); setTimeout(() => { if (jobs.get(key) === rec) { jobs.delete(key); emitJobs(); } }, 6000); throw e; });
}
