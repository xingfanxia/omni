/**
 * Convert a schedule (cron or interval) to a human-readable string.
 */
export function formatSchedule(type: string, value: string): string {
    if (type === 'interval') {
        return formatInterval(parseInt(value))
    }
    return formatCron(value)
}

function formatInterval(seconds: number): string {
    if (seconds >= 86400) {
        const days = Math.floor(seconds / 86400)
        return days === 1 ? 'Every day' : `Every ${days} days`
    }
    if (seconds >= 3600) {
        const hours = Math.floor(seconds / 3600)
        return hours === 1 ? 'Every hour' : `Every ${hours} hours`
    }
    if (seconds >= 60) {
        const minutes = Math.floor(seconds / 60)
        return minutes === 1 ? 'Every minute' : `Every ${minutes} minutes`
    }
    return `Every ${seconds} seconds`
}

const DAYS_OF_WEEK = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday']
const DAYS_SHORT = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat']

function formatCron(expr: string): string {
    const parts = expr.trim().split(/\s+/)
    if (parts.length < 5) return expr

    const [minute, hour, dayOfMonth, month, dayOfWeek] = parts

    // Every N minutes: */N * * * *
    if (hour === '*' && dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
        if (minute === '*') return 'Every minute'
        if (minute.startsWith('*/')) {
            const n = parseInt(minute.slice(2))
            return n === 1 ? 'Every minute' : `Every ${n} minutes`
        }
    }

    // Every N hours: 0 */N * * *
    if (minute === '0' && dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
        if (hour === '*') return 'Every hour'
        if (hour.startsWith('*/')) {
            const n = parseInt(hour.slice(2))
            return n === 1 ? 'Every hour' : `Every ${n} hours`
        }
    }

    const timeStr = formatTime(hour, minute)

    // Daily: M H * * *
    if (dayOfMonth === '*' && month === '*' && dayOfWeek === '*') {
        return `Daily at ${timeStr}`
    }

    // Weekly: M H * * D
    if (dayOfMonth === '*' && month === '*' && dayOfWeek !== '*') {
        const days = parseDayOfWeek(dayOfWeek)
        if (days) return `${days} at ${timeStr}`
    }

    // Monthly: M H D * *
    if (dayOfMonth !== '*' && month === '*' && dayOfWeek === '*') {
        const day = parseInt(dayOfMonth)
        const suffix = getOrdinalSuffix(day)
        return `Monthly on the ${day}${suffix} at ${timeStr}`
    }

    return expr
}

function formatTime(hour: string, minute: string): string {
    const h = parseInt(hour)
    const m = parseInt(minute)
    if (isNaN(h)) return `${hour}:${minute.padStart(2, '0')}`
    const period = h >= 12 ? 'PM' : 'AM'
    const h12 = h === 0 ? 12 : h > 12 ? h - 12 : h
    return m === 0 ? `${h12}${period}` : `${h12}:${String(m).padStart(2, '0')}${period}`
}

function parseDayOfWeek(field: string): string | null {
    // Handle single day: 0-6
    const single = parseInt(field)
    if (!isNaN(single) && single >= 0 && single <= 6) {
        return `Every ${DAYS_OF_WEEK[single]}`
    }

    // Handle comma-separated: 1,3,5
    if (field.includes(',')) {
        const days = field.split(',').map((d) => {
            const n = parseInt(d.trim())
            return !isNaN(n) && n >= 0 && n <= 6 ? DAYS_SHORT[n] : d.trim()
        })
        return `Every ${days.join(', ')}`
    }

    // Handle range: 1-5
    if (field.includes('-')) {
        const [start, end] = field.split('-').map((d) => parseInt(d.trim()))
        if (!isNaN(start) && !isNaN(end) && start >= 0 && end <= 6) {
            return `${DAYS_OF_WEEK[start]}-${DAYS_OF_WEEK[end]}`
        }
    }

    return null
}

function getOrdinalSuffix(n: number): string {
    if (n >= 11 && n <= 13) return 'th'
    switch (n % 10) {
        case 1:
            return 'st'
        case 2:
            return 'nd'
        case 3:
            return 'rd'
        default:
            return 'th'
    }
}
