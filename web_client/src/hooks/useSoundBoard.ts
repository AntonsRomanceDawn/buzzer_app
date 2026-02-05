import { useCallback, useEffect, useRef } from 'react'
import useSound from 'use-sound'

import roundStartWav from '../assets/sounds/round_start.wav'
import winWav from '../assets/sounds/win.wav'
import loseWav from '../assets/sounds/lose.wav'
import timeoutWav from '../assets/sounds/timeout.wav'

export type SoundSettings = {
    roundStart: boolean
    win: boolean
    lose: boolean
    timeout: boolean
}

type SoundBoard = {
    playRoundStart: () => void
    playWin: () => void
    playLose: () => void
    playTimeout: () => void
    prime: () => void
}

type HowlLike = {
    play: () => number | string
    stop: (id?: number | string) => void
    volume: (volume?: number, id?: number | string) => number
}

function primeHowl(sound?: HowlLike | null) {
    if (!sound) return
    const prevVolume = sound.volume()
    sound.volume(0)
    const id = sound.play()
    sound.stop(id)
    sound.volume(prevVolume)
}

export function useSoundBoard(enabled: boolean, settings: SoundSettings): SoundBoard {
    const primedRef = useRef(false)
    const [playRoundStart, roundMeta] = useSound(roundStartWav, {
        volume: 0.3,
        interrupt: true,
        preload: true,
    })
    const [playWin, winMeta] = useSound(winWav, {
        volume: 0.14,
        interrupt: true,
        preload: true,
    })
    const [playLose, loseMeta] = useSound(loseWav, {
        volume: 0.26,
        interrupt: true,
        preload: true,
    })
    const [playTimeout, timeoutMeta] = useSound(timeoutWav, {
        volume: 0.95,
        interrupt: true,
        preload: true,
    })

    useEffect(() => {
        roundMeta?.sound?.load()
        winMeta?.sound?.load()
        loseMeta?.sound?.load()
        timeoutMeta?.sound?.load()
    }, [roundMeta?.sound, winMeta?.sound, loseMeta?.sound, timeoutMeta?.sound])

    const prime = useCallback(() => {
        if (primedRef.current) return
        primeHowl(roundMeta?.sound)
        primeHowl(winMeta?.sound)
        primeHowl(loseMeta?.sound)
        primeHowl(timeoutMeta?.sound)
        primedRef.current = true
    }, [roundMeta?.sound, winMeta?.sound, loseMeta?.sound, timeoutMeta?.sound])

    return {
        playRoundStart: () => {
            if (enabled && settings.roundStart) {
                playRoundStart()
            }
        },
        playWin: () => {
            if (enabled && settings.win) {
                playWin()
            }
        },
        playLose: () => {
            if (enabled && settings.lose) {
                playLose()
            }
        },
        playTimeout: () => {
            if (enabled && settings.timeout) {
                playTimeout()
            }
        },
        prime,
    }
}
