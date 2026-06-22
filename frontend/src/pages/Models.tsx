import { useState, useEffect } from 'react'
import { Plus, Copy, Pencil, Trash2, Check, X, Eye, EyeOff } from 'lucide-react'

type ModelProvider = 'ollama' | 'openai' | 'anthropic' | 'deepseek' | 'custom'
type ConnectionStatus = 'connected' | 'error' | 'testing' | 'untested'
type ModelRole = 'workers' | 'judge' | 'executor'

interface ModelConfig {
  id: string
  name: string
  provider: ModelProvider
  endpoint: string
  modelId: string
  apiKey?: string
  role: ModelRole
  status: ConnectionStatus
  errorMessage?: string
}

interface WorkspaceEntry {
  path: string
  command: string
}

interface GlobalSettings {
  port: number
  maxRetries: number
  verifyTimeout: number
  keepCoreRunning: boolean
  workspaces: WorkspaceEntry[]
}

const ROLE_CONFIG: Record<ModelRole, { title: string; description: string; borderColor: string }> = {
  workers: {
    title: 'Workers',
    description: 'Medium models for parallel diagnosis. 2–3 models recommended.',
    borderColor: 'rgba(34,197,94,0.3)',
  },
  judge: {
    title: 'Judge',
    description: 'High-IQ model that evaluates Worker outputs. 1 model required.',
    borderColor: 'rgba(99,102,241,0.3)',
  },
  executor: {
    title: 'Executor',
    description: 'Fast, low-cost model for mechanical code execution. 1 model required.',
    borderColor: 'rgba(59,130,246,0.3)',
  },
}

const PROVIDER_DEFAULTS: Record<ModelProvider, { endpoint: string; needsKey: boolean }> = {
  ollama: { endpoint: 'http://localhost:11434/v1', needsKey: false },
  openai: { endpoint: 'https://api.openai.com/v1', needsKey: true },
  anthropic: { endpoint: 'https://api.anthropic.com/v1', needsKey: true },
  deepseek: { endpoint: 'https://api.deepseek.com/v1', needsKey: true },
  custom: { endpoint: '', needsKey: false },
}

const PROVIDER_LABELS: Record<ModelProvider, string> = {
  ollama: 'Ollama (Local)',
  openai: 'OpenAI',
  anthropic: 'Anthropic',
  deepseek: 'DeepSeek',
  custom: 'Custom',
}

function generateId() {
  return Math.random().toString(36).slice(2, 10)
}

const tauriInvoke = (cmd: string, args?: Record<string, unknown>): Promise<any> => {
  const t = (window as any).__TAURI__
  const invoke = t?.core?.invoke || t?.invoke
  return invoke ? invoke(cmd, args) : Promise.reject(new Error('Tauri API not available'))
}

function inferProvider(endpoint: string): ModelProvider {
  const ep = endpoint.toLowerCase()
  if (ep.includes('localhost') || ep.includes('ollama') || ep.includes('11434')) return 'ollama'
  if (ep.includes('openai')) return 'openai'
  if (ep.includes('anthropic')) return 'anthropic'
  if (ep.includes('deepseek')) return 'deepseek'
  return 'custom'
}

function configToModels(config: any): ModelConfig[] {
  const models: ModelConfig[] = []
  if (Array.isArray(config.workers)) {
    for (const w of config.workers) {
      if (!w.endpoint || !w.model_id) continue
      models.push({
        id: generateId(),
        name: w.name || '',
        provider: inferProvider(w.endpoint),
        endpoint: w.endpoint,
        modelId: w.model_id,
        apiKey: w.api_key || undefined,
        role: 'workers',
        status: 'untested',
      })
    }
  }
  if (config.judge?.endpoint && config.judge?.model_id) {
    models.push({
      id: generateId(),
      name: config.judge.name || 'judge',
      provider: inferProvider(config.judge.endpoint),
      endpoint: config.judge.endpoint,
      modelId: config.judge.model_id,
      apiKey: config.judge.api_key || undefined,
      role: 'judge',
      status: 'untested',
    })
  }
  if (config.executor?.endpoint && config.executor?.model_id) {
    models.push({
      id: generateId(),
      name: config.executor.name || 'executor',
      provider: inferProvider(config.executor.endpoint),
      endpoint: config.executor.endpoint,
      modelId: config.executor.model_id,
      apiKey: config.executor.api_key || undefined,
      role: 'executor',
      status: 'untested',
    })
  }
  return models
}

function modelsToConfig(models: ModelConfig[], baseConfig: any): any {
  const workers = models.filter((m) => m.role === 'workers')
  const judges = models.filter((m) => m.role === 'judge')
  const executors = models.filter((m) => m.role === 'executor')
  return {
    port: baseConfig?.port ?? 9999,
    workers: workers.map((w) => ({
      name: w.name,
      endpoint: w.endpoint,
      model_id: w.modelId,
      api_key: w.apiKey ?? null,
    })),
    judge: judges.length > 0
      ? { name: judges[0].name, endpoint: judges[0].endpoint, model_id: judges[0].modelId, api_key: judges[0].apiKey ?? null }
      : { name: 'judge', endpoint: 'http://localhost:11434', model_id: 'llama3', api_key: null },
    executor: executors.length > 0
      ? { name: executors[0].name, endpoint: executors[0].endpoint, model_id: executors[0].modelId, api_key: executors[0].apiKey ?? null }
      : { name: 'executor', endpoint: 'http://localhost:11434', model_id: 'llama3', api_key: null },
    workspaces: baseConfig?.workspaces ?? {},
    error_keywords: baseConfig?.error_keywords ?? [],
  }
}

function ModelStatusDot({ status }: { status: ConnectionStatus }) {
  const colors: Record<ConnectionStatus, string> = {
    connected: 'var(--status-active)',
    error: 'var(--status-error)',
    testing: 'var(--status-warning)',
    untested: 'var(--status-inactive)',
  }
  const labels: Record<ConnectionStatus, string> = {
    connected: 'Connected',
    error: 'Disconnected',
    testing: 'Testing...',
    untested: 'Untested',
  }
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
      <span
        style={{
          width: 8,
          height: 8,
          borderRadius: 'var(--radius-full)',
          backgroundColor: colors[status],
          animation: status === 'testing' ? 'status-pulse 2s ease-in-out infinite' : 'none',
        }}
      />
      <span style={{ fontSize: '0.75rem', color: 'var(--text-secondary)', fontWeight: 500 }}>
        {labels[status]}
      </span>
    </div>
  )
}

function ModelCard({
  model,
  onEdit,
  onDelete,
  onTest,
}: {
  model: ModelConfig
  onEdit: () => void
  onDelete: () => void
  onTest: () => void
}) {
  const [menuOpen, setMenuOpen] = useState(false)
  const [showKey, setShowKey] = useState(false)
  const [copied, setCopied] = useState(false)

  const copyEndpoint = () => {
    navigator.clipboard.writeText(`${model.endpoint}/chat/completions`)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }

  return (
    <div
      style={{
        background: 'var(--bg-secondary)',
        border: '1px solid var(--border-subtle)',
        borderRadius: 'var(--radius-lg)',
        padding: '16px',
        minHeight: 96,
        cursor: 'pointer',
      }}
      onMouseEnter={(e) => { e.currentTarget.style.background = 'var(--bg-hover)' }}
      onMouseLeave={(e) => { e.currentTarget.style.background = 'var(--bg-secondary)' }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontWeight: 600, fontSize: '0.9375rem', color: 'var(--text-primary)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
            {model.provider}/{model.modelId}
          </div>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: '0.8125rem', color: 'var(--text-secondary)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
            {model.endpoint}
          </div>
        </div>
        <div style={{ position: 'relative', display: 'flex', gap: 6, alignItems: 'center', flexShrink: 0, marginLeft: 12 }}>
          <ModelStatusDot status={model.status} />
          <button
            onClick={onTest}
            style={{
              padding: '4px 10px',
              fontSize: '0.75rem',
              background: 'var(--bg-tertiary)',
              border: '1px solid var(--border-primary)',
              borderRadius: 'var(--radius-md)',
              color: 'var(--text-primary)',
              cursor: 'pointer',
            }}
          >
            Test
          </button>
          <div style={{ position: 'relative' }}>
            <button
              onClick={() => setMenuOpen(!menuOpen)}
              style={{
                background: 'transparent',
                border: 'none',
                color: 'var(--text-secondary)',
                cursor: 'pointer',
                padding: 4,
                borderRadius: 'var(--radius-md)',
              }}
            >
              ⋯
            </button>
            {menuOpen && (
              <>
                <div style={{ position: 'fixed', inset: 0, zIndex: 9 }} onClick={() => setMenuOpen(false)} />
                <div style={{
                  position: 'absolute', right: 0, top: '100%', zIndex: 10,
                  background: 'var(--bg-secondary)', border: '1px solid var(--border-primary)',
                  borderRadius: 'var(--radius-md)', padding: 4, minWidth: 120, boxShadow: '0 4px 12px rgba(0,0,0,0.3)',
                }}>
                  <button onClick={() => { setMenuOpen(false); onEdit() }} style={menuItemStyle}>
                    <Pencil size={14} /> Edit
                  </button>
                  <button onClick={() => { setMenuOpen(false); onDelete() }} style={{ ...menuItemStyle, color: 'var(--status-error)' }}>
                    <Trash2 size={14} /> Delete
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      </div>

      {model.status === 'error' && model.errorMessage && (
        <div style={{ fontSize: '0.75rem', color: 'var(--status-error)', marginBottom: 8 }}>
          {model.errorMessage}
        </div>
      )}

      <div style={{ display: 'flex', alignItems: 'center', gap: 8, fontSize: '0.75rem', color: 'var(--text-tertiary)' }}>
        {model.apiKey && (
          <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
            Key: {showKey ? model.apiKey : '••••••••'}
            <button onClick={() => setShowKey(!showKey)} style={iconBtnStyle}>
              {showKey ? <EyeOff size={12} /> : <Eye size={12} />}
            </button>
          </span>
        )}
        <button onClick={copyEndpoint} style={{ ...iconBtnStyle, marginLeft: 'auto' }}>
          {copied ? <Check size={12} color="var(--status-active)" /> : <Copy size={12} />}
        </button>
      </div>
    </div>
  )
}

const menuItemStyle: React.CSSProperties = {
  display: 'flex', alignItems: 'center', gap: 8,
  width: '100%', padding: '6px 10px', fontSize: '0.8125rem',
  background: 'transparent', border: 'none', color: 'var(--text-primary)',
  cursor: 'pointer', borderRadius: 'var(--radius-sm)',
}

const iconBtnStyle: React.CSSProperties = {
  background: 'transparent', border: 'none', color: 'var(--text-tertiary)',
  cursor: 'pointer', padding: 2, display: 'flex',
}

function ModelDialog({
  open,
  model,
  onSave,
  onClose,
}: {
  open: boolean
  model: ModelConfig | null
  onSave: (m: ModelConfig) => void
  onClose: () => void
}) {
  const [provider, setProvider] = useState<ModelProvider>(model?.provider ?? 'ollama')
  const [displayName, setDisplayName] = useState(model?.name ?? '')
  const [endpoint, setEndpoint] = useState(model?.endpoint ?? '')
  const [modelId, setModelId] = useState(model?.modelId ?? '')
  const [apiKey, setApiKey] = useState(model?.apiKey ?? '')
  const [testOnSave, setTestOnSave] = useState(true)
  const [errors, setErrors] = useState<Record<string, string>>({})

  const currentProvider = PROVIDER_DEFAULTS[provider]

  if (!open) return null

  const validate = (): boolean => {
    const e: Record<string, string> = {}
    if (!displayName.trim()) e.displayName = 'Display name is required'
    if (!endpoint.trim()) e.endpoint = 'Endpoint URL is required'
    else if (!/^https?:\/\/.+/.test(endpoint)) e.endpoint = 'Must be a valid http/https URL'
    if (!modelId.trim()) e.modelId = 'Model ID is required'
    if (currentProvider.needsKey && !apiKey.trim() && !endpoint.includes('localhost')) {
      e.apiKey = 'API key recommended for cloud endpoints'
    }
    setErrors(e)
    return Object.keys(e).length === 0
  }

  const handleSave = () => {
    if (!validate()) return
    onSave({
      id: model?.id ?? generateId(),
      name: displayName,
      provider,
      endpoint,
      modelId,
      apiKey: apiKey || undefined,
      role: model?.role ?? 'workers',
      status: 'untested',
    })
    onClose()
  }

  return (
    <>
      <div style={{ position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.6)', backdropFilter: 'blur(4px)', zIndex: 49 }} onClick={onClose} />
      <div style={{
        position: 'fixed', top: '50%', left: '50%', transform: 'translate(-50%, -50%)',
        background: 'var(--bg-secondary)', border: '1px solid var(--border-subtle)',
        borderRadius: 'var(--radius-xl)', padding: 24, maxWidth: 520, width: '90%', zIndex: 50,
      }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 20 }}>
          <h2 style={{ fontSize: '1.125rem', fontWeight: 600 }}>
            {model ? 'Edit Model' : 'Add Model'}
          </h2>
          <button onClick={onClose} style={{ background: 'transparent', border: 'none', color: 'var(--text-secondary)', cursor: 'pointer' }}>
            <X size={18} />
          </button>
        </div>

        <div style={{ display: 'flex', gap: 4, marginBottom: 16, flexWrap: 'wrap' }}>
          {(Object.keys(PROVIDER_LABELS) as ModelProvider[]).map((p) => (
            <button
              key={p}
              onClick={() => {
                setProvider(p)
                const d = PROVIDER_DEFAULTS[p]
                setEndpoint(d.endpoint)
                setErrors({})
              }}
              style={{
                padding: '4px 12px', fontSize: '0.75rem', fontWeight: 500,
                background: provider === p ? 'rgba(99,102,241,0.15)' : 'var(--bg-tertiary)',
                border: `1px solid ${provider === p ? 'var(--accent-primary)' : 'var(--border-primary)'}`,
                borderRadius: 'var(--radius-full)',
                color: provider === p ? 'var(--accent-primary)' : 'var(--text-secondary)',
                cursor: 'pointer',
              }}
            >
              {PROVIDER_LABELS[p]}
            </button>
          ))}
        </div>

        {(['displayName', 'endpoint', 'modelId', 'apiKey'] as const).map((field) => (
          <div key={field} style={{ marginBottom: 12 }}>
            <label style={{ fontSize: '0.8125rem', fontWeight: 500, color: 'var(--text-secondary)', display: 'block', marginBottom: 4 }}>
              {field === 'displayName' ? 'Display Name' : field === 'modelId' ? 'Model ID' : field === 'apiKey' ? 'API Key (optional)' : 'Endpoint URL'}
            </label>
            <input
              type={field === 'apiKey' ? 'password' : 'text'}
              value={
                field === 'displayName' ? displayName :
                field === 'endpoint' ? endpoint :
                field === 'modelId' ? modelId : apiKey
              }
              onChange={(e) => {
                const val = e.target.value
                if (field === 'displayName') setDisplayName(val)
                else if (field === 'endpoint') setEndpoint(val)
                else if (field === 'modelId') setModelId(val)
                else setApiKey(val)
                setErrors({})
              }}
              style={{
                width: '100%', height: 36, padding: '8px 12px',
                background: 'var(--bg-tertiary)', border: `1px solid ${errors[field] ? 'var(--status-error)' : 'var(--border-primary)'}`,
                borderRadius: 'var(--radius-md)', color: 'var(--text-primary)', fontSize: '0.875rem',
                fontFamily: field === 'endpoint' ? 'var(--font-mono)' : 'var(--font-sans)',
              }}
              placeholder={
                field === 'displayName' ? 'e.g. qwen2.5-coder:7b' :
                field === 'endpoint' ? 'http://localhost:11434/v1' :
                field === 'modelId' ? 'qwen2.5-coder:7b' : 'sk-...'
              }
            />
            {errors[field] && (
              <div style={{ fontSize: '0.75rem', color: 'var(--status-error)', marginTop: 2 }}>{errors[field]}</div>
            )}
          </div>
        ))}

        <label style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 16, cursor: 'pointer' }}>
          <input type="checkbox" checked={testOnSave} onChange={(e) => setTestOnSave(e.target.checked)} />
          <span style={{ fontSize: '0.8125rem', color: 'var(--text-secondary)' }}>Test connection after saving</span>
        </label>

        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 12 }}>
          <button onClick={onClose}
            style={{
              padding: '8px 16px', fontSize: '0.8125rem', fontWeight: 500,
              background: 'transparent', border: 'none', color: 'var(--text-secondary)', cursor: 'pointer',
            }}>
            Cancel
          </button>
          <button onClick={handleSave}
            style={{
              padding: '8px 20px', fontSize: '0.8125rem', fontWeight: 500,
              background: 'var(--accent-primary)', border: 'none',
              borderRadius: 'var(--radius-md)', color: '#fff', cursor: 'pointer',
            }}>
            Save
          </button>
        </div>
      </div>
    </>
  )
}

function RoleSection({
  role,
  models,
  onAdd,
  onEdit,
  onDelete,
  onTest,
}: {
  role: ModelRole
  models: ModelConfig[]
  onAdd: () => void
  onEdit: (m: ModelConfig) => void
  onDelete: (m: ModelConfig) => void
  onTest: (m: ModelConfig) => void
}) {
  const cfg = ROLE_CONFIG[role]

  return (
    <div style={{
      background: 'var(--bg-secondary)',
      border: '1px solid var(--border-subtle)',
      borderRadius: 'var(--radius-lg)',
      padding: 20,
      borderLeft: `3px solid ${cfg.borderColor}`,
      marginBottom: 32,
    }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
        <div>
          <h3 style={{ fontSize: '0.9375rem', fontWeight: 600, color: 'var(--text-primary)' }}>
            {cfg.title}
          </h3>
          <p style={{ fontSize: '0.8125rem', color: 'var(--text-secondary)', marginTop: 2 }}>
            {cfg.description}
          </p>
        </div>
        <button
          onClick={onAdd}
          style={{
            padding: '6px 14px', fontSize: '0.8125rem', fontWeight: 500,
            background: 'var(--accent-primary)', border: 'none',
            borderRadius: 'var(--radius-md)', color: '#fff', cursor: 'pointer',
            display: 'flex', alignItems: 'center', gap: 6,
          }}
        >
          <Plus size={16} /> Add
        </button>
      </div>

      {models.length === 0 ? (
        <div style={{ padding: '24px 0', textAlign: 'center', color: 'var(--text-tertiary)', fontSize: '0.8125rem' }}>
          No {cfg.title.toLowerCase()} configured.
          <br />
          <span style={{ color: 'var(--text-secondary)' }}>
            {role === 'workers' ? 'Workers run in parallel during diagnosis.' :
             role === 'judge' ? 'The Judge synthesizes Worker outputs into a plan.' :
             'The Executor handles fast code generation.'}
          </span>
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          {models.map((m) => (
            <ModelCard
              key={m.id}
              model={m}
              onEdit={() => onEdit(m)}
              onDelete={() => onDelete(m)}
              onTest={() => onTest(m)}
            />
          ))}
        </div>
      )}
    </div>
  )
}

export function Models() {
  const [models, setModels] = useState<ModelConfig[]>([])
  const [fullConfig, setFullConfig] = useState<any>(null)
  const [configLoaded, setConfigLoaded] = useState(false)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [editingModel, setEditingModel] = useState<ModelConfig | null>(null)
  const [addingRole, setAddingRole] = useState<ModelRole>('workers')
  const [settings, setSettings] = useState<GlobalSettings>({
    port: 9999, maxRetries: 3, verifyTimeout: 45, keepCoreRunning: true, workspaces: [],
  })
  const [newWsPath, setNewWsPath] = useState('')
  const [newWsCmd, setNewWsCmd] = useState('')

  // Load config from disk on mount
  useEffect(() => {
    if (typeof window !== 'undefined' && (window as any).__TAURI__) {
      tauriInvoke('get_config').then((config: any) => {
        setFullConfig(config)
        setModels(configToModels(config))
        setConfigLoaded(true)
      }).catch(() => {
        setConfigLoaded(true)
      })
    } else {
      setConfigLoaded(true)
    }
  }, [])

  // Persist models to config.json after every change
  const persistModels = async (updatedModels: ModelConfig[]) => {
    if (typeof window !== 'undefined' && (window as any).__TAURI__) {
      try {
        const merged = modelsToConfig(updatedModels, fullConfig)
        await tauriInvoke('save_config', { config: merged })
        setFullConfig(merged)
      } catch (e) {
        console.error('Failed to save model config:', e)
      }
    }
  }

  const filteredRole = (role: ModelRole) => models.filter((m) => m.role === role)

  const handleAdd = (role: ModelRole) => {
    setAddingRole(role)
    setEditingModel(null)
    setDialogOpen(true)
  }

  const handleEdit = (model: ModelConfig) => {
    setEditingModel({ ...model })
    setDialogOpen(true)
  }

  const handleSave = (model: ModelConfig) => {
    let updated: ModelConfig[]
    if (editingModel) {
      updated = models.map((m) => (m.id === model.id ? model : m))
    } else {
      updated = [...models, { ...model, role: addingRole }]
    }
    setModels(updated)
    persistModels(updated)
  }

  const handleDelete = (model: ModelConfig) => {
    if (confirm(`Remove ${model.name}?`)) {
      const updated = models.filter((m) => m.id !== model.id)
      setModels(updated)
      persistModels(updated)
    }
  }

  const handleTest = async (model: ModelConfig) => {
    setModels(models.map((m) => {
      if (m.id === model.id) return { ...m, status: 'testing' as ConnectionStatus }
      return m
    }))
    try {
      const headers: Record<string, string> = { 'Content-Type': 'application/json' }
      if (model.apiKey) {
        headers['Authorization'] = `Bearer ${model.apiKey}`
      }
      const resp = await fetch(`${model.endpoint}/chat/completions`, {
        method: 'POST',
        headers,
        body: JSON.stringify({ model: model.modelId, messages: [{ role: 'user', content: 'test' }], max_tokens: 1 }),
        signal: AbortSignal.timeout(10000),
      })
      setModels(models.map((m) => {
        if (m.id === model.id) return { ...m, status: resp.ok ? 'connected' : 'error', errorMessage: resp.ok ? undefined : `HTTP ${resp.status}` }
        return m
      }))
    } catch (e) {
      setModels(models.map((m) => {
        if (m.id === model.id) return { ...m, status: 'error', errorMessage: (e as Error).message }
        return m
      }))
    }
  }

  const addWorkspace = () => {
    if (newWsPath.trim() && newWsCmd.trim()) {
      setSettings({
        ...settings,
        workspaces: [...settings.workspaces, { path: newWsPath, command: newWsCmd }],
      })
      setNewWsPath('')
      setNewWsCmd('')
    }
  }

  return (
    <div style={{ padding: 24, maxWidth: 960 }}>
      {!configLoaded ? null : (
      <>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 24 }}>
        <h1 style={{ fontSize: '1.5rem', fontWeight: 600, color: 'var(--text-primary)' }}>
          Model Configuration
        </h1>
      </div>

      {models.length === 0 ? (
        <div style={{
          textAlign: 'center', padding: '64px 0', background: 'var(--bg-secondary)',
          border: '1px solid var(--border-subtle)', borderRadius: 'var(--radius-lg)',
        }}>
          <div style={{ fontSize: 48, marginBottom: 16, opacity: 0.3 }}>⚡</div>
          <h2 style={{ fontSize: '1.125rem', fontWeight: 600, marginBottom: 8 }}>No Models Configured Yet</h2>
          <p style={{ color: 'var(--text-secondary)', marginBottom: 6 }}>Add your first model to start using the AI gateway.</p>
          <p style={{ color: 'var(--text-tertiary)', fontSize: '0.8125rem' }}>
            You'll need at minimum: 1 Executor (fast model) · For Fusion: 2+ Workers + 1 Judge
          </p>
        </div>
      ) : (
        <>
          <RoleSection role="workers" models={filteredRole('workers')} onAdd={() => handleAdd('workers')} onEdit={handleEdit} onDelete={handleDelete} onTest={handleTest} />
          <RoleSection role="judge" models={filteredRole('judge')} onAdd={() => handleAdd('judge')} onEdit={handleEdit} onDelete={handleDelete} onTest={handleTest} />
          <RoleSection role="executor" models={filteredRole('executor')} onAdd={() => handleAdd('executor')} onEdit={handleEdit} onDelete={handleDelete} onTest={handleTest} />
        </>
      )}

      <div style={{
        background: 'var(--bg-secondary)', border: '1px solid var(--border-subtle)',
        borderRadius: 'var(--radius-lg)', padding: 20, marginTop: 32,
      }}>
        <h3 style={{ fontSize: '0.9375rem', fontWeight: 600, marginBottom: 16 }}>Global Settings</h3>
        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(200px, 1fr))', gap: 16 }}>
          <div>
            <label style={{ fontSize: '0.8125rem', fontWeight: 500, color: 'var(--text-secondary)', display: 'block', marginBottom: 4 }}>Gateway Port</label>
            <input type="number" value={settings.port}
              onChange={(e) => setSettings({ ...settings, port: parseInt(e.target.value) || 9999 })}
              style={inputStyle} />
          </div>
          <div>
            <label style={{ fontSize: '0.8125rem', fontWeight: 500, color: 'var(--text-secondary)', display: 'block', marginBottom: 4 }}>Max Oracle Retries</label>
            <input type="number" value={settings.maxRetries} min={1} max={10}
              onChange={(e) => setSettings({ ...settings, maxRetries: parseInt(e.target.value) || 3 })}
              style={inputStyle} />
          </div>
          <div>
            <label style={{ fontSize: '0.8125rem', fontWeight: 500, color: 'var(--text-secondary)', display: 'block', marginBottom: 4 }}>Verify Timeout (s)</label>
            <input type="number" value={settings.verifyTimeout}
              onChange={(e) => setSettings({ ...settings, verifyTimeout: parseInt(e.target.value) || 45 })}
              style={inputStyle} />
          </div>
          <div>
            <label style={{ fontSize: '0.8125rem', fontWeight: 500, color: 'var(--text-secondary)', display: 'block', marginBottom: 4 }}>Keep Core Running</label>
            <button
              onClick={() => setSettings({ ...settings, keepCoreRunning: !settings.keepCoreRunning })}
              style={{
                width: 44, height: 24, borderRadius: 'var(--radius-full)',
                background: settings.keepCoreRunning ? 'var(--accent-primary)' : 'var(--bg-tertiary)',
                border: 'none', cursor: 'pointer', position: 'relative',
              }}>
              <span style={{
                position: 'absolute', top: 2,
                left: settings.keepCoreRunning ? 22 : 2,
                width: 20, height: 20, borderRadius: '50%',
                background: '#fff', transition: 'left 150ms ease-out',
              }} />
            </button>
          </div>
        </div>

        <div style={{ marginTop: 20 }}>
          <h4 style={{ fontSize: '0.8125rem', fontWeight: 500, color: 'var(--text-secondary)', marginBottom: 8 }}>
            Workspace Verification Commands
          </h4>
          {settings.workspaces.map((ws, i) => (
            <div key={i} style={{ display: 'flex', gap: 8, marginBottom: 8, alignItems: 'center' }}>
              <code style={{ fontFamily: 'var(--font-mono)', fontSize: '0.75rem', color: 'var(--text-tertiary)', flex: 1, padding: '4px 8px', background: 'var(--bg-tertiary)', borderRadius: 'var(--radius-sm)' }}>
                {ws.path}: {ws.command}
              </code>
              <button onClick={() => setSettings({ ...settings, workspaces: settings.workspaces.filter((_, j) => j !== i) })}
                style={{ background: 'transparent', border: 'none', color: 'var(--status-error)', cursor: 'pointer' }}>
                <X size={14} />
              </button>
            </div>
          ))}
          <div style={{ display: 'flex', gap: 8 }}>
            <input value={newWsPath} onChange={(e) => setNewWsPath(e.target.value)} placeholder="Workspace path" style={{ ...inputStyle, flex: 1 }} />
            <input value={newWsCmd} onChange={(e) => setNewWsCmd(e.target.value)} placeholder="Verify command" style={{ ...inputStyle, flex: 1 }} />
            <button onClick={addWorkspace} style={{
              padding: '6px 14px', fontSize: '0.75rem', background: 'var(--accent-primary)', border: 'none',
              borderRadius: 'var(--radius-md)', color: '#fff', cursor: 'pointer',
            }}>
              <Plus size={14} />
            </button>
          </div>
        </div>
      </div>

      <ModelDialog
        open={dialogOpen}
        model={editingModel}
        onSave={handleSave}
        onClose={() => setDialogOpen(false)}
      />
      </>
      )}
    </div>
  )
}

const inputStyle: React.CSSProperties = {
  width: '100%', height: 36, padding: '8px 12px',
  background: 'var(--bg-tertiary)', border: '1px solid var(--border-primary)',
  borderRadius: 'var(--radius-md)', color: 'var(--text-primary)', fontSize: '0.875rem',
}
