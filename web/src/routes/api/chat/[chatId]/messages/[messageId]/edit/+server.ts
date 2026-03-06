import { json } from '@sveltejs/kit'
import type { RequestHandler } from './$types.js'
import { chatRepository, chatMessageRepository } from '$lib/server/db/chats'

interface EditRequest {
    content: string
}

export const POST: RequestHandler = async ({ params, request, locals }) => {
    const logger = locals.logger.child('chat')

    const { chatId, messageId } = params
    if (!chatId || !messageId) {
        return json({ error: 'chatId and messageId are required' }, { status: 400 })
    }

    if (!locals.user?.id) {
        return json({ error: 'User not authenticated' }, { status: 401 })
    }

    let editRequest: EditRequest
    try {
        editRequest = await request.json()
    } catch {
        return json({ error: 'Invalid JSON in request body' }, { status: 400 })
    }

    if (!editRequest.content || editRequest.content.trim() === '') {
        return json({ error: 'Content is required' }, { status: 400 })
    }

    try {
        const chat = await chatRepository.get(chatId)
        if (!chat) {
            return json({ error: 'Chat not found' }, { status: 404 })
        }

        // Get the original message to find its parent
        const allMessages = await chatMessageRepository.getByChatId(chatId)
        const originalMessage = allMessages.find((m) => m.id === messageId)
        if (!originalMessage) {
            return json({ error: 'Message not found' }, { status: 404 })
        }

        // Create new message as a sibling of the original (same parent)
        const userMessage = {
            role: 'user' as const,
            content: editRequest.content.trim(),
        }

        const savedMessage = await chatMessageRepository.create(
            chatId,
            userMessage,
            originalMessage.parentId ?? undefined,
        )

        logger.info('Message edited (new branch created)', {
            chatId,
            originalMessageId: messageId,
            newMessageId: savedMessage.id,
        })

        return json(
            {
                messageId: savedMessage.id,
                status: 'created',
            },
            { status: 200 },
        )
    } catch (error) {
        logger.error('Error editing message', error, { chatId, messageId })
        return json(
            {
                error: 'Failed to edit message',
                details: error instanceof Error ? error.message : 'Unknown error',
            },
            { status: 500 },
        )
    }
}
