import { useState, useEffect, useRef, useCallback } from 'react'
import { ChevronDown, ArrowDown } from 'lucide-react'

type SessionState = 'Diagnostic' | 'Execution' | 'Verify' | 'Retry' | 'Done' | 'Failed'
type LogPhase = 'diag' | 'exec' | 'veri' | 'retry' | 'info'

interface LogEntry {
  id: string
  timestamp: string
  phase: LogPhase
  sessionId: string
  message: string
}

interface SessionStats {
  workers: number
  duration: string
  requests: string
  tokens: number
}

interface SessionInfo {
  id: string
  sessionName: string
  state: SessionState
  retryCount?: number
  maxRetries?: number
  stats: SessionStats
  lastError?: string
  suggestedAction?: string
}

const STATE_CONFIG: Record<SessionState, { color: string; borderColor: string; pulse: boolean }> = {
  Diagnostic: { color: 'var(--status-active)', borderColor: 'rgba(34,197,94,0.3)', pulse: true },
  Execution: { color: 'var(--status-info)', borderColor: 'rgba(59,130,246,0.3)', pulse: false },
  Verify: { color: 'var(--status-warning)', borderColor: 'rgba(245,158,11,0.3)', pulse: false },
  Retry: { color: 'var(--status-error)', borderColor: 'rgba(239,68,68,0.3)', pulse: true },
  Done: { color: 'var(--status-active)', borderColor: 'rgba(34,197,94,0.3)', pulse: false },
  Failed: { color: 'var(--status-error)', borderColor: 'rgba(239,68,68,0.3)', pulse: false },
}

const PHASE_COLORS: Record<LogPhase, { bg: string; text: string; border: string }> = {
  diag: { bg: 'rgba(34,197,94,0.1)', text: 'var(--status-active)', border: 'rgba(34,197,94,0.3)' },
  exec: { bg: 'rgba(59,130,246,0.1)', text: 'var(--status-info)', border: 'rgba(59,130,246,0.3)' },
  veri: { bg: 'rgba(245,158,11,0.1)', text: 'var(--status-warning)', border: 'rgba(245,158,11,0.3)' },
  retry: { bg: 'rgba(239,68,68,0.1)', text: 'var(--status-error)', border: 'rgba(239,68,68,0.3)' },
  info: { bg: 'var(--bg-tertiary)', text: 'var(--text-secondary)', border: 'var(--border-primary)' },
}

function SessionCard({ session, onEnd }: { session: SessionInfo; onEnd: (id: string) => void }) {
  const cfg = STATE_CONFIG[session.state]
  const opacity = session.state === 'Done' ? 0.85 : 1
  const stateLabel = session.state === 'Retry' && session.retryCount !== undefined
    ? `Retry ${session.retryCount}/${session.maxRetries ?? 3}` : session.state

  return (
    <div style={{
      background: 'var(--bg-secondary)', border: '1px solid var(--border-subtle)',
      borderRadius: 'var(--radius-lg)', padding: 20, opacity,
      borderLeft: `3px solid ${cfg.borderColor}`, minHeight: 160, display: 'flex', flexDirection: 'column',
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span style={{ width: 8, height: 8, borderRadius: 'var(--radius-full)', backgroundColor: cfg.color, animation: cfg.pulse ? 'status-pulse 2s ease-in-out infinite' : 'none' }} />
          <span style={{ fontSize: '0.75rem', fontWeight: 600, textTransform: 'uppercase', letterSpacing: '0.05em', color: cfg.color, background: `${cfg.color}15`, padding: '3px 10px', borderRadius: 'var(--radius-full)' }}>
            {stateLabel}
          </span>
        </div>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: '0.75rem', color: 'var(--text-tertiary)' }}>{session.sessionName}</span>
      </div>
      {session.state === 'Retry' && session.lastError && (
        <div style={{ marginBottom: 12, padding: 8, background: 'var(--bg-tertiary)', borderRadius: 'var(--radius-sm)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: '0.75rem', color: 'var(--text-secondary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            Last error: {session.lastError}
          </div>
          {session.suggestedAction && <div style={{ fontSize: '0.75rem', color: 'var(--text-tertiary)' }}>{session.suggestedAction}</div>}
        </div>
      )}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 8, fontSize: '0.8125rem', color: 'var(--text-secondary)' }}>
        <div>Workers: <span style={{ color: 'var(--text-primary)', fontWeight: 500 }}>{session.stats.workers}</span></div>
        <div>Duration: <span style={{ color: 'var(--text-primary)', fontWeight: 500, fontFamily: 'var(--font-mono)' }}>{session.stats.duration}</span></div>
        <div>Requests: <span style={{ color: 'var(--text-primary)', fontWeight: 500, fontFamily: 'var(--font-mono)' }}>{session.stats.requests}</span></div>
        <div>Tokens: <span style={{ color: 'var(--text-primary)', fontWeight: 500, fontFamily: 'var(--font-mono)' }}>{session.stats.tokens.toLocaleString()}</span></div>
      </div>
      <div style={{ marginTop: 'auto', paddingTop: 16 }}>
        {session.state === 'Failed' ? (
          <button onClick={() => onEnd(session.id)} style={dangerBtnStyle}>Retry</button>
        ) : (
          <button onClick={() => onEnd(session.id)} style={{ ...dangerBtnStyle, opacity: 0.7 }}>End Session</button>
        )}
      </div>
    </div>
  )
}

function LogPanel({ entries }: { entries: LogEntry[] }) {
  const [autoScroll, setAutoScroll] = useState(true)
  const scrollRef = useRef<HTMLDivElement>(null)
  const userScrolled = useRef(false)

  useEffect(() => {
    if (autoScroll && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [entries, autoScroll])

  const handleScroll = useCallback(() => {
    if (!scrollRef.current) return
    const { scrollTop, scrollHeight, clientHeight } = scrollRef.current
    if (scrollHeight - scrollTop - clientHeight < 30) { setAutoScroll(true); userScrolled.current = false }
    else { userScrolled.current = true; setAutoScroll(false) }
  }, [])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', flex: 1, minHeight: 200, maxHeight: '60vh', position: 'relative' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: '8px 16px', background: 'var(--bg-secondary)', borderBottom: '1px solid var(--border-subtle)', position: 'sticky', top: 0, zIndex: 5 }}>
        <span style={{ fontSize: '0.8125rem', fontWeight: 600, color: 'var(--text-primary)' }}>Live Activity</span>
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <button onClick={() => setAutoScroll(!autoScroll)} style={{ ...ghostBtnStyle, opacity: autoScroll ? 1 : 0.5 }}><ChevronDown size={14} /></button>
        </div>
      </div>
      <div ref={scrollRef} onScroll={handleScroll} style={{ flex: 1, overflowY: 'auto', background: 'var(--bg-primary)' }}>
        {entries.length === 0 ? (
          <div style={{ padding: '40px 0', textAlign: 'center', color: 'var(--text-tertiary)', fontSize: '0.8125rem' }}>
            Waiting for activity...<br /><span style={{ fontSize: '0.75rem' }}>Sessions will appear when AI tools connect to the gateway.</span>
          </div>
        ) : (
          entries.map((entry) => {
            const pc = PHASE_COLORS[entry.phase]
            return (
              <div key={entry.id} style={{ display: 'flex', gap: 8, alignItems: 'baseline', padding: '8px 16px', fontSize: '0.8125rem', fontFamily: 'var(--font-mono)', borderBottom: '1px solid var(--border-subtle)', background: entries.indexOf(entry) % 2 === 0 ? 'var(--bg-primary)' : 'var(--bg-secondary)' }}>
                <span style={{ color: 'var(--text-tertiary)', fontSize: '0.75rem', minWidth: 80, fontVariantNumeric: 'tabular-nums' }}>{entry.timestamp}</span>
                <span style={{ background: pc.bg, color: pc.text, border: `1px solid ${pc.border}`, padding: '1px 6px', borderRadius: 'var(--radius-sm)', fontSize: '0.6875rem', fontWeight: 600, textTransform: 'uppercase', minWidth: 40, textAlign: 'center' }}>{entry.phase}</span>
                <span style={{ color: 'var(--text-tertiary)', fontSize: '0.75rem', minWidth: 80 }}>{entry.sessionId}</span>
                <span style={{ color: 'var(--text-secondary)' }}>{entry.message}</span>
              </div>
            )
          })
        )}
      </div>
      {!autoScroll && entries.length > 0 && (
        <button onClick={() => { setAutoScroll(true); userScrolled.current = false }} style={{ position: 'absolute', bottom: 8, right: 8, zIndex: 10, background: 'var(--accent-primary)', border: 'none', borderRadius: 'var(--radius-full)', color: '#fff', width: 32, height: 32, cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          <ArrowDown size={16} />
        </button>
      )}
    </div>
  )
}

export function Dashboard() {
  const [sessions, setSessions] = useState<SessionInfo[]>([])
  const [logEntries, setLogEntries] = useState<LogEntry[]>([])

  const fetchSessions = () => {
    fetch('http://localhost:9999/v1/sessions')
      .then((res) => res.json())
      .then((data) => {
        const mapped = data.map((s: { id: string; name: string; state: string; retryCount: number; maxRetries: number; duration: string; requests: number; tokens: number }) => ({
          id: s.id,
          sessionName: s.name,
          state: s.state as SessionState,
          retryCount: s.retryCount,
          maxRetries: s.maxRetries,
          stats: {
            workers: s.state === 'Diagnostic' ? 2 : 0,
            duration: s.duration,
            requests: `${s.requests}/∞`,
            tokens: s.tokens,
          },
        }))
        setSessions(mapped)
      })
      .catch((err) => console.error('Failed to fetch sessions:', err))
  }

  const endSession = async (id: string) => {
    if (confirm('Are you sure you want to end this session?')) {
      try {
        await fetch(`http://localhost:9999/v1/sessions/${id}`, { method: 'DELETE' })
        fetchSessions()
      } catch (e) {
        alert('Failed to end session: ' + e)
      }
    }
  }

  useEffect(() => {
    fetchSessions()
    const interval = setInterval(fetchSessions, 3000)

    // 1. 先加载部分初始历史日志（来自 metrics）
    fetch('http://localhost:9999/v1/metrics')
      .then((res) => res.json())
      .then((data) => {
        const logs = data.slice(-10).map((m: { request_id: string; timestamp: number; panel_failure_count: number; judge_model: string; total_latency_ms: number; consensus_count: number; contradiction_count: number }) => {
          const date = new Date(m.timestamp * 1000)
          return {
            id: m.request_id + '-init',
            timestamp: date.toTimeString().slice(0, 8),
            phase: (m.panel_failure_count > 0 ? 'retry' : 'diag') as LogPhase,
            sessionId: `session-${m.request_id.slice(0, 4)}`,
            message: `Deliberation complete via ${m.judge_model}. Latency: ${m.total_latency_ms}ms. Consensus: ${m.consensus_count}, Contradictions: ${m.contradiction_count}`,
          }
        })
        setLogEntries(logs)
      })
      .catch(() => {})

    // 2. 然后建立 SSE 实时通道监听事件
    const eventSource = new EventSource('http://localhost:9999/v1/events')

    eventSource.onmessage = (event) => {
      try {
        const parsed = JSON.parse(event.data) as { timestamp: number; event_type: string; message: string; session_id?: string }
        const date = new Date(parsed.timestamp * 1000)
        const timestamp = date.toTimeString().slice(0, 8)

        let phase: LogPhase = 'info'
        if (parsed.event_type === 'fusion') phase = 'diag'
        else if (parsed.event_type === 'execution') phase = 'exec'
        else if (parsed.event_type === 'verify') phase = 'veri'
        else if (parsed.event_type === 'error') phase = 'retry'

        const newEntry: LogEntry = {
          id: parsed.timestamp + '-' + Math.random().toString(36).slice(2, 7),
          timestamp,
          phase,
          sessionId: parsed.session_id ? `session-${parsed.session_id.slice(0, 4)}` : 'gateway',
          message: parsed.message,
        }

        setLogEntries((prev) => [...prev, newEntry])
      } catch (e) {
        console.error(e)
      }
    }

    return () => {
      clearInterval(interval)
      eventSource.close()
    }
  }, [])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', overflow: 'hidden' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', padding: '12px 24px', borderBottom: '1px solid var(--border-subtle)', background: 'var(--bg-primary)', position: 'sticky', top: 0, zIndex: 10, minHeight: 56 }}>
        <div>
          <h1 style={{ fontSize: '1.5rem', fontWeight: 600, color: 'var(--text-primary)' }}>Dashboard</h1>
          <div style={{ fontSize: '0.75rem', color: 'var(--text-tertiary)', display: 'flex', gap: 12, marginTop: 2 }}>
            <span><span style={{ color: 'var(--status-active)', fontWeight: 600 }}>●</span> Core Running</span>
            <span>Active: {sessions.length}</span>
            <span>Memory: 12.4 MB</span>
          </div>
        </div>
      </div>
      <div style={{ flex: 1, overflowY: 'auto', padding: 24 }}>
        {sessions.length === 0 ? (
          <div style={{ padding: '40px 0', textAlign: 'center', color: 'var(--text-tertiary)', fontSize: '0.8125rem', background: 'var(--bg-secondary)', borderRadius: 'var(--radius-lg)', border: '1px dashed var(--border-primary)', marginBottom: 24 }}>
            No Active Sessions<br />
            <span style={{ fontSize: '0.75rem' }}>Send requests to http://localhost:9999/v1/chat/completions</span>
          </div>
        ) : (
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))', gap: 24, marginBottom: 24 }}>
            {sessions.map((s) => <SessionCard key={s.id} session={s} onEnd={endSession} />)}
          </div>
        )}
        <LogPanel entries={logEntries} />
      </div>
    </div>
  )
}

const dangerBtnStyle: React.CSSProperties = {
  background: 'transparent', border: '1px solid rgba(239,68,68,0.3)',
  color: 'var(--status-error)', fontSize: '0.8125rem', fontWeight: 500,
  padding: '6px 16px', borderRadius: 'var(--radius-md)', cursor: 'pointer', width: '100%',
}

const ghostBtnStyle: React.CSSProperties = {
  background: 'transparent', border: 'none', color: 'var(--text-secondary)', cursor: 'pointer', padding: 4, display: 'flex',
}
