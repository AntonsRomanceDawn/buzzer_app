import { useEffect, useRef, useState } from 'react'
import './App.css'

type Role = 'admin' | 'player'

type CreateRoomResponse = {
  room_id: string
  token: string
  answer_window_in_ms: number
}

type JoinRoomResponse = {
  token: string | null
}

type RefreshTokenResponse = {
  token: string
}

type ParticipantInfo = {
  name: string
  role: Role
}

type ServerMessage =
  | { type: 'accepted'; name: string; deadline_ms: number }
  | { type: 'rejected' }
  | { type: 'timed_out'; name: string }
  | { type: 'round_started' }
  | { type: 'participants'; participants: ParticipantInfo[] }
  | { type: 'action_denied'; reason: string }
  | { type: 'kicked' }

type LogEntry = {
  id: string
  ts: string
  text: string
  tone?: 'ok' | 'warn' | 'bad'
}

const TOKEN_STORAGE_KEY = 'bg_tokens'
const NAME_STORAGE_KEY = 'bg_names'
const REFRESH_THRESHOLD_MS = 15 * 60 * 1000
const REFRESH_CHECK_INTERVAL_MS = 5 * 60 * 1000

function loadMap(key: string): Record<string, string> {
  try {
    const raw = localStorage.getItem(key)
    if (!raw) return {}
    const parsed = JSON.parse(raw) as Record<string, string>
    return parsed ?? {}
  } catch {
    return {}
  }
}

function saveMap(key: string, value: Record<string, string>) {
  localStorage.setItem(key, JSON.stringify(value))
}

function getJwtExpMs(token: string): number | null {
  try {
    const [, payload] = token.split('.')
    if (!payload) return null
    const normalized = payload.replace(/-/g, '+').replace(/_/g, '/')
    const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=')
    const decoded = atob(padded)
    const data = JSON.parse(decoded) as { exp?: number }
    if (!data.exp) return null
    return data.exp * 1000
  } catch {
    return null
  }
}

function App() {
  const [roomId, setRoomId] = useState('')
  const [name, setName] = useState('')
  const [role, setRole] = useState<Role | null>(null)
  const [participants, setParticipants] = useState<ParticipantInfo[]>([])
  const [token, setToken] = useState<string | null>(null)
  const [status, setStatus] = useState<'idle' | 'loading' | 'ready'>('idle')
  const [view, setView] = useState<'landing' | 'room'>('landing')
  const [error, setError] = useState<string | null>(null)
  const [wsState, setWsState] = useState<'disconnected' | 'connecting' | 'connected'>(
    'disconnected'
  )
  const [logs, setLogs] = useState<LogEntry[]>([])
  const [adminTarget, setAdminTarget] = useState('')
  const [kickTarget, setKickTarget] = useState('')
  const [result, setResult] = useState<'idle' | 'won' | 'lost' | 'rejected'>('idle')
  const [myName, setMyName] = useState('')
  const [roomMode, setRoomMode] = useState<'create' | 'join'>('create')
  const [answerWindowMs, setAnswerWindowMs] = useState('5000')
  const wsRef = useRef<WebSocket | null>(null)

  useEffect(() => {
    if (!roomId) return
    const savedTokens = loadMap(TOKEN_STORAGE_KEY)
    const savedNames = loadMap(NAME_STORAGE_KEY)
    const saved = savedTokens[roomId]
    if (saved) {
      setToken(saved)
    }
    const savedName = savedNames[roomId]
    if (savedName) {
      setName(savedName)
      setMyName(savedName)
    }
  }, [roomId])

  const appendLog = (text: string, tone: LogEntry['tone'] = 'ok') => {
    const ts = new Date().toLocaleTimeString()
    setLogs((prev) => [
      { id: crypto.randomUUID(), ts, text, tone },
      ...prev.slice(0, 200),
    ])
  }

  const persistAuth = (room: string, nextToken: string, nextName?: string) => {
    const tokens = loadMap(TOKEN_STORAGE_KEY)
    tokens[room] = nextToken
    saveMap(TOKEN_STORAGE_KEY, tokens)
    if (nextName) {
      const names = loadMap(NAME_STORAGE_KEY)
      names[room] = nextName
      saveMap(NAME_STORAGE_KEY, names)
    }
  }

  const refreshTokenIfNeeded = async (
    currentToken: string | null,
    room: string
  ): Promise<string | null> => {
    if (!currentToken || !room) return currentToken
    const expMs = getJwtExpMs(currentToken)
    if (!expMs) return currentToken
    if (expMs - Date.now() > REFRESH_THRESHOLD_MS) {
      return currentToken
    }
    try {
      const resp = await fetch(`/api/rooms/${room}/refresh_token`, {
        method: 'POST',
        headers: { Authorization: `Bearer ${currentToken}` },
      })
      if (!resp.ok) {
        return currentToken
      }
      const data = (await resp.json()) as RefreshTokenResponse
      if (!data.token) return currentToken
      setToken(data.token)
      persistAuth(room, data.token)
      return data.token
    } catch {
      return currentToken
    }
  }

  const resetSession = () => {
    setRole(null)
    setParticipants([])
    setToken(null)
    setStatus('idle')
    setWsState('disconnected')
    setLogs([])
    setResult('idle')
    if (wsRef.current) {
      wsRef.current.close()
      wsRef.current = null
    }
    setView('landing')
  }

  const createRoom = async () => {
    setError(null)
    setStatus('loading')
    try {
      const resp = await fetch('/api/rooms', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name,
          answer_window_in_ms: Number.isFinite(Number(answerWindowMs))
            ? Number(answerWindowMs)
            : null,
        }),
      })
      if (!resp.ok) {
        const text = await resp.text()
        throw new Error(text || 'failed_to_create_room')
      }
      const data = (await resp.json()) as CreateRoomResponse
      setRoomId(data.room_id)
      setMyName(name)
      setRole('admin')
      setToken(data.token)
      setAnswerWindowMs(String(data.answer_window_in_ms))
      setStatus('ready')
      setView('room')
      appendLog('Room created. You are the admin.', 'ok')
      persistAuth(data.room_id, data.token, name)
    } catch (err) {
      setError((err as Error).message)
      setStatus('idle')
    }
  }

  const joinRoom = async () => {
    setError(null)
    setStatus('loading')
    try {
      const savedTokens = loadMap(TOKEN_STORAGE_KEY)
      const savedToken = savedTokens[roomId]
      if (savedToken) {
        setToken(savedToken)
      }
      const headers: Record<string, string> = {}
      let body: string | undefined

      if (savedToken) {
        headers.Authorization = `Bearer ${savedToken}`
      } else {
        headers['Content-Type'] = 'application/json'
        body = JSON.stringify({ name: name.trim() || null })
      }

      const resp = await fetch(`/api/rooms/${roomId}/join`, {
        method: 'POST',
        headers,
        body,
      })

      if (!resp.ok) {
        const text = await resp.text()
        throw new Error(text || 'failed_to_join')
      }

      const data = (await resp.json()) as JoinRoomResponse
      if (!data.token) {
        throw new Error('missing_token')
      }
      const nextRole = savedToken ? role ?? 'player' : 'player'
      setRole(nextRole)
      setToken(data.token)
      setStatus('ready')
      setView('room')
      appendLog('Joined room.', 'ok')
      if (name.trim()) {
        setMyName(name.trim())
      }
      persistAuth(roomId, data.token, name)
    } catch (err) {
      setError((err as Error).message)
      setStatus('idle')
    }
  }

  const connectWs = async () => {
    if (!token || !roomId) {
      setError('missing_token_or_room')
      return
    }
    if (wsRef.current) {
      wsRef.current.close()
    }
    setWsState('connecting')
    const freshToken = await refreshTokenIfNeeded(token, roomId)
    if (!freshToken) {
      setError('missing_token_or_room')
      setWsState('disconnected')
      return
    }
    const wsUrl = `${window.location.origin.replace('http', 'ws')}/ws/${roomId}?token=${freshToken}`
    const ws = new WebSocket(wsUrl)
    wsRef.current = ws

    ws.onopen = () => {
      setWsState('connected')
      appendLog('WebSocket connected.', 'ok')
      setResult('idle')
    }
    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data) as ServerMessage
        switch (msg.type) {
          case 'accepted':
            appendLog(`${msg.name} buzzed first.`, 'ok')
            setResult(msg.name === myName ? 'won' : 'lost')
            break
          case 'rejected':
            appendLog('Buzz rejected.', 'warn')
            setResult('rejected')
            break
          case 'timed_out':
            appendLog(`${msg.name} timed out.`, 'bad')
            setResult('idle')
            break
          case 'round_started':
            appendLog('Round started.', 'ok')
            setResult('idle')
            break
          case 'participants': {
            setParticipants(msg.participants)
            const identity = myName || name
            if (identity) {
              const me = msg.participants.find((participant) => participant.name === identity)
              if (me) {
                setRole(me.role)
              } else if (view === 'room') {
                appendLog('You are no longer in the room.', 'bad')
                resetSession()
                return
              }
            }
            appendLog('Participants updated.', 'ok')
            break
          }
          case 'action_denied':
            appendLog(`Action denied: ${msg.reason}`, 'warn')
            break
          case 'kicked':
            appendLog('You were removed from the room.', 'bad')
            setResult('idle')
            resetSession()
            break
          default:
            appendLog('Unknown server message.', 'warn')
        }
      } catch {
        appendLog('Invalid message from server.', 'warn')
      }
    }
    ws.onclose = () => {
      setWsState('disconnected')
      appendLog('WebSocket disconnected.', 'warn')
    }
    ws.onerror = () => {
      setWsState('disconnected')
      appendLog('WebSocket error.', 'bad')
    }
  }

  const sendBuzz = () => {
    if (wsRef.current?.readyState !== WebSocket.OPEN) {
      appendLog('WebSocket not connected.', 'warn')
      return
    }
    wsRef.current.send(JSON.stringify({ type: 'buzz' }))
  }

  const startRound = () => {
    if (wsRef.current?.readyState !== WebSocket.OPEN) {
      appendLog('WebSocket not connected.', 'warn')
      return
    }
    wsRef.current.send(JSON.stringify({ type: 'start_round' }))
  }

  const setAdmin = () => {
    if (wsRef.current?.readyState !== WebSocket.OPEN) {
      appendLog('WebSocket not connected.', 'warn')
      return
    }
    if (!adminTarget.trim()) {
      appendLog('Admin target required.', 'warn')
      return
    }
    wsRef.current.send(JSON.stringify({ type: 'set_admin', name: adminTarget.trim() }))
    setAdminTarget('')
  }

  const kickPlayer = () => {
    if (wsRef.current?.readyState !== WebSocket.OPEN) {
      appendLog('WebSocket not connected.', 'warn')
      return
    }
    if (!kickTarget.trim()) {
      appendLog('Kick target required.', 'warn')
      return
    }
    wsRef.current.send(JSON.stringify({ type: 'kick', name: kickTarget.trim() }))
    setKickTarget('')
  }

  useEffect(() => {
    if (view !== 'room') return
    if (wsState === 'connected' || wsState === 'connecting') return
    void connectWs()
  }, [view, wsState])

  useEffect(() => {
    if (view !== 'room' || !token || !roomId) return
    const interval = setInterval(() => {
      void refreshTokenIfNeeded(token, roomId)
    }, REFRESH_CHECK_INTERVAL_MS)
    return () => clearInterval(interval)
  }, [view, token, roomId])

  return (
    <div className="app">
      <header className="hero">
        <div>
          <p className="eyebrow">buzzer_game</p>
          <h1>Fastest finger wins.</h1>
          <p className="subhead">
            Create a room, share the code, and run lightning rounds in your browser.
          </p>
        </div>
        <div className="hero-meta">
          <div>
            <span className="label">Connection</span>
            <span className={`pill ${wsState}`}>{wsState}</span>
          </div>
          <div>
            <span className="label">Room ID</span>
            <span className="pill">{roomId || '—'}</span>
          </div>
          <div>
            <span className="label">Role</span>
            <span className="pill">{role ?? '—'}</span>
          </div>
          <div>
            <span className="label">Answer window</span>
            <span className="pill">
              {answerWindowMs ? `${answerWindowMs} ms` : '—'}
            </span>
          </div>
        </div>
      </header>

      {view === 'landing' && (
        <main className="layout">
          <section className="panel wide">
            <div className="panel-row">
              <div>
                <h2>Start or join</h2>
                <p className="panel-sub">
                  Choose your path. Saved identity is used automatically if available.
                </p>
              </div>
              <div className="panel-actions">
                <button
                  className={roomMode === 'create' ? 'secondary' : 'ghost'}
                  onClick={() => setRoomMode('create')}
                >
                  Create room
                </button>
                <button
                  className={roomMode === 'join' ? 'secondary' : 'ghost'}
                  onClick={() => setRoomMode('join')}
                >
                  Join room
                </button>
              </div>
            </div>

            {roomMode === 'create' ? (
              <div className="split">
                <label>
                  Display name
                  <input
                    value={name}
                    onChange={(event) => setName(event.target.value)}
                    placeholder="e.g. Ada"
                  />
                </label>
                <label>
                  Answer window (ms)
                  <input
                    type="number"
                    value={answerWindowMs}
                    onChange={(event) => setAnswerWindowMs(event.target.value)}
                    placeholder="5000"
                  />
                </label>
                <button disabled={status === 'loading' || !name.trim()} onClick={createRoom}>
                  Create room
                </button>
              </div>
            ) : (
              <div className="split">
                <label>
                  Room id
                  <input
                    value={roomId}
                    onChange={(event) => setRoomId(event.target.value.trim())}
                    placeholder="room id"
                  />
                </label>
                <label>
                  Display name (optional)
                  <input
                    value={name}
                    onChange={(event) => setName(event.target.value)}
                    placeholder="only if new"
                  />
                </label>
                <button disabled={status === 'loading' || !roomId} onClick={joinRoom}>
                  Join room
                </button>
              </div>
            )}

            {error && <div className="error">Error: {error}</div>}
          </section>
        </main>
      )}

      {view === 'room' && (
        <main className="layout">
          <section className="panel wide">
            <div className="panel-row">
              <div>
                <h2>Room console</h2>
                <p className="panel-sub">Manage the round and send buzzes.</p>
              </div>
              <div className="panel-actions">
                <button className="ghost" onClick={resetSession}>
                  Leave room
                </button>
              </div>
            </div>

            <div className={`result ${result}`}>
              <h3>
                {result === 'idle' && 'Ready to buzz'}
                {result === 'won' && 'You won the buzz!'}
                {result === 'lost' && 'Too slow! Someone else buzzed first.'}
                {result === 'rejected' && 'Buzz rejected'}
              </h3>
              <p>
                {result === 'idle' && 'Wait for the round to start, then buzz fast.'}
                {result === 'won' && 'You are up. Answer quickly!'}
                {result === 'lost' && 'Try again next round.'}
                {result === 'rejected' && 'You already buzzed or are locked out.'}
              </p>
            </div>

            <div className="room-grid">
              <div className="room-card">
                <h3>Participants</h3>
                <ul>
                  {participants.length === 0 && <li className="muted">No players yet.</li>}
                  {participants.map((participant) => (
                    <li key={participant.name}>
                      <span>{participant.name}</span>
                      <span className={`badge ${participant.role}`}>{participant.role}</span>
                    </li>
                  ))}
                </ul>
              </div>
              <div className="room-card">
                <h3>Controls</h3>
                <div className="controls">
                  <button onClick={sendBuzz} disabled={wsState !== 'connected'}>
                    Buzz
                  </button>
                  <button
                    className="secondary"
                    onClick={startRound}
                    disabled={role !== 'admin' || wsState !== 'connected'}
                  >
                    Start round
                  </button>
                </div>
                <div className="controls admin-tools">
                  <label>
                    Set admin
                    <input
                      value={adminTarget}
                      onChange={(event) => setAdminTarget(event.target.value)}
                      placeholder="player name"
                      disabled={role !== 'admin' || wsState !== 'connected'}
                    />
                  </label>
                  <button
                    className="ghost"
                    onClick={setAdmin}
                    disabled={role !== 'admin' || wsState !== 'connected'}
                  >
                    Assign admin
                  </button>
                </div>
                <div className="controls admin-tools">
                  <label>
                    Kick player
                    <input
                      value={kickTarget}
                      onChange={(event) => setKickTarget(event.target.value)}
                      placeholder="player name"
                      disabled={role !== 'admin' || wsState !== 'connected'}
                    />
                  </label>
                  <button
                    className="ghost"
                    onClick={kickPlayer}
                    disabled={role !== 'admin' || wsState !== 'connected'}
                  >
                    Kick
                  </button>
                </div>
                <p className="hint">
                  Admin can start rounds. Everyone can buzz once connected.
                </p>
              </div>
              <div className="room-card logs">
                <h3>Event log</h3>
                <div className="log-list">
                  {logs.length === 0 && <p className="muted">No events yet.</p>}
                  {logs.map((entry) => (
                    <div key={entry.id} className={`log ${entry.tone ?? 'ok'}`}>
                      <span>{entry.ts}</span>
                      <span>{entry.text}</span>
                    </div>
                  ))}
                </div>
              </div>
            </div>

            {error && <div className="error">Error: {error}</div>}
          </section>
        </main>
      )}
    </div>
  )
}

export default App
