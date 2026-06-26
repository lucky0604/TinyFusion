import { useState, useMemo, useEffect } from 'react'
import { Search, X, Download } from 'lucide-react'

type LogPhase = 'diag' | 'exec' | 'veri' | 'retry' | 'info'

interface MetricDetail {
  request_id: string
  timestamp: number
  total_latency_ms: number
  outer_model: string
  panel_models: string[]
  judge_model: string
  panel_latencies_ms: number[]
  judge_latency_ms: number
  refiner_latency_ms: number
  consensus_count: number
  contradiction_count: number
  blind_spot_count: number
  panel_success_count: number
  panel_failure_count: number
}

interface LogEntry {
  id: string
  timestamp: string
  phase: LogPhase
  sessionId: string
  message: string
  requestSize?: number
  workers?: string
  judge?: string
  rawMetrics?: MetricDetail
}

const PHASE_COLORS: Record<LogPhase, { bg: string; text: string; border: string }> = {
  diag: { bg: 'rgba(34,197,94,0.1)', text: 'var(--status-active)', border: 'rgba(34,197,94,0.3)' },
  exec: { bg: 'rgba(59,130,246,0.1)', text: 'var(--status-info)', border: 'rgba(59,130,246,0.3)' },
  veri: { bg: 'rgba(245,158,11,0.1)', text: 'var(--status-warning)', border: 'rgba(245,158,11,0.3)' },
  retry: { bg: 'rgba(239,68,68,0.1)', text: 'var(--status-error)', border: 'rgba(239,68,68,0.3)' },
  info: { bg: 'var(--bg-tertiary)', text: 'var(--text-secondary)', border: 'var(--border-primary)' },
}

export function Logs() {
  const [logs, setLogs] = useState<LogEntry[]>([])
  const [search, setSearch] = useState('')
  const [phaseFilter, setPhaseFilter] = useState<Set<LogPhase>>(new Set())
  const [sessionFilter, setSessionFilter] = useState<Set<string>>(new Set())
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [exportOpen, setExportOpen] = useState(false)

  const fetchLogs = () => {
    fetch('http://localhost:9999/v1/metrics')
      .then((res) => res.json())
      .then((data) => {
        const mapped = data.map((m: MetricDetail) => {
          const date = new Date(m.timestamp * 1000)
          return {
            id: m.request_id,
            timestamp: date.toTimeString().slice(0, 8),
            phase: (m.panel_failure_count > 0 ? 'retry' : 'diag') as LogPhase,
            sessionId: `session-${m.request_id.slice(0, 4)}`,
            message: `Deliberation complete via ${m.judge_model}. Latency: ${m.total_latency_ms}ms. Consensus: ${m.consensus_count}, Contradictions: ${m.contradiction_count}`,
            requestSize: m.total_latency_ms > 2000 ? 3200 : 1200,
            workers: m.panel_models.join(', '),
            judge: m.judge_model,
            rawMetrics: m,
          }
        })
        mapped.reverse()
        setLogs(mapped)
      })
      .catch((err) => {
        console.error('Failed to fetch logs:', err)
      })
  }

  useEffect(() => {
    fetchLogs()
  }, [])

  const sessions = useMemo(() => [...new Set(logs.map((l) => l.sessionId))], [logs])

  const togglePhase = (p: LogPhase) => {
    const next = new Set(phaseFilter)
    if (next.has(p)) {
      next.delete(p)
    } else {
      next.add(p)
    }
    setPhaseFilter(next)
  }
  const toggleSession = (s: string) => {
    const next = new Set(sessionFilter)
    if (next.has(s)) {
      next.delete(s)
    } else {
      next.add(s)
    }
    setSessionFilter(next)
  }
  const clearFilters = () => { setSearch(''); setPhaseFilter(new Set()); setSessionFilter(new Set()) }

  const filteredLogs = logs.filter((l) => {
    if (search && !l.message.toLowerCase().includes(search.toLowerCase())) return false
    if (phaseFilter.size > 0 && !phaseFilter.has(l.phase)) return false
    if (sessionFilter.size > 0 && !sessionFilter.has(l.sessionId)) return false
    return true
  })

  const hasFilters = search || phaseFilter.size > 0 || sessionFilter.size > 0

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', overflow: 'hidden' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: '12px 24px', borderBottom: '1px solid var(--border-subtle)', background: 'var(--bg-primary)' }}>
        <h1 style={{ fontSize: '1.5rem', fontWeight: 600 }}>Logs</h1>
        <button onClick={() => setExportOpen(true)} style={{ display: 'flex', alignItems: 'center', gap: 6, padding: '6px 14px', fontSize: '0.8125rem', fontWeight: 500, background: 'var(--bg-tertiary)', border: '1px solid var(--border-primary)', borderRadius: 'var(--radius-md)', color: 'var(--text-primary)', cursor: 'pointer' }}>
          <Download size={14} /> Export
        </button>
      </div>
      <div style={{ padding: '12px 24px', background: 'var(--bg-secondary)', borderBottom: '1px solid var(--border-subtle)' }}>
        <div style={{ position: 'relative', marginBottom: 8 }}>
          <Search size={14} style={{ position: 'absolute', left: 10, top: 10, color: 'var(--text-tertiary)' }} />
          <input value={search} onChange={(e) => setSearch(e.target.value)} placeholder="Search logs..." style={{ width: '100%', height: 36, padding: '8px 12px 8px 32px', background: 'var(--bg-tertiary)', border: '1px solid var(--border-primary)', borderRadius: 'var(--radius-md)', color: 'var(--text-primary)', fontSize: '0.8125rem' }} />
          {search && <button onClick={() => setSearch('')} style={{ position: 'absolute', right: 10, top: 10, background: 'none', border: 'none', color: 'var(--text-tertiary)', cursor: 'pointer' }}><X size={14} /></button>}
        </div>
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', alignItems: 'center' }}>
          {(['diag', 'exec', 'veri', 'retry', 'info'] as LogPhase[]).map((p) => (
            <label key={p} style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: '0.75rem', color: 'var(--text-secondary)', cursor: 'pointer' }}>
              <input type="checkbox" checked={phaseFilter.has(p)} onChange={() => togglePhase(p)} /> {p}
            </label>
          ))}
          <span style={{ color: 'var(--border-primary)' }}>|</span>
          {sessions.slice(0, 5).map((s) => (
            <label key={s} style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: '0.75rem', color: 'var(--text-secondary)', cursor: 'pointer' }}>
              <input type="checkbox" checked={sessionFilter.has(s)} onChange={() => toggleSession(s)} /> {s}
            </label>
          ))}
          {hasFilters && <button onClick={clearFilters} style={{ fontSize: '0.75rem', color: 'var(--accent-primary)', background: 'none', border: 'none', cursor: 'pointer', marginLeft: 'auto' }}>Clear all filters</button>}
        </div>
        {hasFilters && (
          <div style={{ display: 'flex', gap: 4, marginTop: 6, flexWrap: 'wrap' }}>
            {search && <FilterPill label={`"${search}"`} onRemove={() => setSearch('')} />}
            {[...phaseFilter].map((p) => <FilterPill key={p} label={p} onRemove={() => togglePhase(p)} />)}
            {[...sessionFilter].map((s) => <FilterPill key={s} label={s} onRemove={() => toggleSession(s)} />)}
          </div>
        )}
      </div>
      <div style={{ flex: 1, overflowY: 'auto' }}>
        {filteredLogs.length === 0 ? (
          <div style={{ padding: '60px 0', textAlign: 'center', color: 'var(--text-tertiary)', fontSize: '0.8125rem' }}>
            {logs.length === 0 ? <>No log entries yet.<br /><span style={{ fontSize: '0.75rem' }}>Activity will appear when AI tools connect.</span></> :
              <><p style={{ marginBottom: 12 }}>No entries match your filters.</p><button onClick={clearFilters} style={{ padding: '6px 16px', background: 'var(--accent-primary)', border: 'none', borderRadius: 'var(--radius-md)', color: '#fff', cursor: 'pointer', fontSize: '0.8125rem' }}>Clear All Filters</button></>}
          </div>
        ) : (
          filteredLogs.map((entry, i) => {
            const pc = PHASE_COLORS[entry.phase]
            const isExpanded = expandedId === entry.id
            return (
              <div key={entry.id}>
                <div onClick={() => setExpandedId(isExpanded ? null : entry.id)}
                  style={{ display: 'flex', gap: 8, alignItems: 'baseline', padding: '8px 16px', fontSize: '0.8125rem', fontFamily: 'var(--font-mono)', height: 32, borderBottom: '1px solid var(--border-subtle)', background: i % 2 === 0 ? 'var(--bg-primary)' : 'var(--bg-secondary)', cursor: 'pointer' }}>
                  <span style={{ color: 'var(--text-tertiary)', fontSize: '0.75rem', minWidth: 120, fontVariantNumeric: 'tabular-nums' }}>{entry.timestamp}</span>
                  <span style={{ background: pc.bg, color: pc.text, border: `1px solid ${pc.border}`, padding: '1px 4px', borderRadius: 'var(--radius-sm)', fontSize: '0.6875rem', fontWeight: 600, textTransform: 'uppercase', minWidth: 64, textAlign: 'center' }}>{entry.phase}</span>
                  <span style={{ color: 'var(--text-tertiary)', fontSize: '0.75rem', minWidth: 120, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{entry.sessionId}</span>
                  <span style={{ color: 'var(--text-secondary)', flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{entry.message}</span>
                </div>
                {isExpanded && (
                  <div style={{ padding: '12px 16px', background: 'var(--bg-tertiary)', borderBottom: '1px solid var(--border-subtle)', fontSize: '0.75rem', color: 'var(--text-secondary)', fontFamily: 'var(--font-mono)' }}>
                    <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12, marginBottom: 12 }}>
                      <div>Session ID: <span style={{ color: 'var(--text-primary)' }}>{entry.id}</span></div>
                      <div>Judge: <span style={{ color: 'var(--text-primary)' }}>{entry.judge}</span></div>
                      <div>Workers: <span style={{ color: 'var(--text-primary)' }}>{entry.workers}</span></div>
                      <div>Total Latency: <span style={{ color: 'var(--text-primary)' }}>{entry.rawMetrics?.total_latency_ms}ms</span></div>
                    </div>
                    <details style={{ marginTop: 8 }}>
                      <summary style={{ cursor: 'pointer', color: 'var(--accent-primary)', marginBottom: 6 }}>View Full Metrics JSON</summary>
                      <pre style={{ padding: 12, background: 'var(--bg-primary)', borderRadius: 'var(--radius-md)', overflowX: 'auto', maxHeight: 200, fontSize: '0.7rem' }}>
                        {JSON.stringify(entry.rawMetrics, null, 2)}
                      </pre>
                    </details>
                  </div>
                )}
              </div>
            )
          })
        )}
      </div>
      <div style={{ padding: '8px 16px', borderTop: '1px solid var(--border-subtle)', fontSize: '0.75rem', color: 'var(--text-tertiary)', background: 'var(--bg-secondary)' }}>
        Showing {filteredLogs.length} of {logs.length} entries
      </div>
      {exportOpen && (
        <>
          <div style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.6)', backdropFilter: 'blur(4px)', zIndex: 49 }} onClick={() => setExportOpen(false)} />
          <div style={{ position: 'fixed', top: '50%', left: '50%', transform: 'translate(-50%, -50%)', background: 'var(--bg-secondary)', border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-xl)', padding: 24, maxWidth: 400, width: '90%', zIndex: 50 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 16 }}>
              <h2 style={{ fontSize: '1.125rem', fontWeight: 600 }}>Export Logs</h2>
              <button onClick={() => setExportOpen(false)} style={{ background: 'none', border: 'none', color: 'var(--text-secondary)', cursor: 'pointer' }}><X size={18} /></button>
            </div>
            <p style={{ fontSize: '0.8125rem', color: 'var(--text-secondary)', marginBottom: 16 }}>Estimated entries: {filteredLogs.length}</p>
            <div style={{ display: 'flex', gap: 8, marginBottom: 16 }}>
              {['JSON', 'CSV', 'Plain Text'].map((f) => (
                <button key={f} onClick={() => { alert(`Export as ${f}`); setExportOpen(false) }} style={{ flex: 1, padding: '8px', fontSize: '0.8125rem', fontWeight: 500, background: 'var(--bg-tertiary)', border: '1px solid var(--border-primary)', borderRadius: 'var(--radius-md)', color: 'var(--text-primary)', cursor: 'pointer' }}>{f}</button>
              ))}
            </div>
            <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
              <button onClick={() => setExportOpen(false)} style={{ padding: '8px 20px', fontSize: '0.8125rem', fontWeight: 500, background: 'var(--accent-primary)', border: 'none', borderRadius: 'var(--radius-md)', color: '#fff', cursor: 'pointer' }}>Close</button>
            </div>
          </div>
        </>
      )}
    </div>
  )
}

function FilterPill({ label, onRemove }: { label: string; onRemove: () => void }) {
  return (
    <span onClick={onRemove} style={{ display: 'inline-flex', alignItems: 'center', gap: 4, padding: '2px 8px', fontSize: '0.6875rem', background: 'var(--bg-tertiary)', border: '1px solid var(--border-primary)', borderRadius: 'var(--radius-full)', color: 'var(--text-secondary)', cursor: 'pointer' }}>
      {label} <X size={10} />
    </span>
  )
}
