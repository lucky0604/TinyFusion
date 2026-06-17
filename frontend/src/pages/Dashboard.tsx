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

function SessionCard({ session }: { session: SessionInfo }) {
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
          <button style={dangerBtnStyle}>Retry</button>
        ) : (
          <button style={{ ...dangerBtnStyle, opacity: 0.7 }}>End Session</button>
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
  const [logEntries] = useState<LogEntry[]>(() => {
    const t = new Date().toISOString().slice(11, 19)
    return [
      { id: '1', timestamp: t, phase: 'diag', sessionId: 'session-a3f2', message: 'Diagnostic phase started, spawning 3 workers' },
      { id: '2', timestamp: t, phase: 'diag', sessionId: 'session-a3f2', message: 'Worker qwen-coder responded' },
      { id: '3', timestamp: t, phase: 'diag', sessionId: 'session-a3f2', message: 'Judge analyzing results...' },
      { id: '4', timestamp: t, phase: 'exec', sessionId: 'session-a3f2', message: 'Passthrough → deepseek-chat' },
      { id: '5', timestamp: t, phase: 'exec', sessionId: 'session-a3f2', message: 'Tool call: apply_changes' },
      { id: '6', timestamp: t, phase: 'veri', sessionId: 'session-a3f2', message: 'Running: cargo build...' },
    ]
  })

  const sessions: SessionInfo[] = [
    { id: 'a3f2', sessionName: '#a3f2', state: 'Diagnostic', stats: { workers: 3, duration: '4.2s', requests: '1/∞', tokens: 1247 } },
    { id: 'b7k1', sessionName: '#b7k1', state: 'Execution', stats: { workers: 0, duration: '2.1s', requests: '6/∞', tokens: 3420 } },
    { id: 'c9m3', sessionName: '#c9m3', state: 'Verify', stats: { workers: 0, duration: '8.5s', requests: '3/∞', tokens: 2100 } },
  ]

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
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))', gap: 24, marginBottom: 24 }}>
          {sessions.map((s) => <SessionCard key={s.id} session={s} />)}
        </div>
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
