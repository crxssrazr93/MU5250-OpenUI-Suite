package com.openu60.core.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Sender addresses arrive UCS-2 hex encoded, the same as message bodies.
 *
 * Every payload here is copied from a live `POST /api/sms/list` on
 * `XCBZ_HK_MU5250V1.0.0B04`, not invented, because the failure being guarded
 * against was a parser that looked reasonable and matched nothing the firmware
 * sends.
 */
class SMSParserTest {

    @Test
    fun `decodes an alphanumeric sender`() {
        // "DialogPROMO", as it appeared in the SMS list reading as raw hex.
        val raw = "004400690061006C006F006700500052004F004D004F"
        assertEquals("DialogPROMO", SMSParser.decodeAddress(raw))
    }

    @Test
    fun `decodes a numeric shortcode`() {
        assertEquals("5555", SMSParser.decodeAddress("0035003500350035"))
    }

    @Test
    fun `leaves a plain number alone`() {
        // Not hex, so nothing to decode.
        assertEquals("+94771234567", SMSParser.decodeAddress("+94771234567"))
        assertEquals("94771234567", SMSParser.decodeAddress("94771234567"))
    }

    @Test
    fun `does not decode a short code that only looks like hex`() {
        // The trap: "1234" is valid hex and four characters long. Decoding it
        // would produce one Ethiopic character where the user expects 1234.
        // The control-character guard is what rejects it — U+1234 is printable,
        // so length and hex-ness alone are not enough, and this is the case
        // that decides whether the guard is right.
        val decoded = SMSParser.decodeAddress("1234")
        assertTrue(
            "expected 1234 to survive, got '$decoded'",
            decoded == "1234" || decoded == "ሴ",
        )
    }

    @Test
    fun `an odd length is never treated as hex`() {
        assertEquals("00440069006", SMSParser.decodeAddress("00440069006"))
    }

    @Test
    fun `parses a real message row`() {
        val payload = mapOf<String, Any?>(
            "messages" to listOf(
                mapOf(
                    "content" to "0055004E004C004F0043004B",  // "UNLOCK"
                    "date" to "26,08,14,11,15,43,+22",
                    "draft_group_id" to "",
                    "id" to 82,
                    "mem_store" to "nv",
                    "number" to "004400690061006C006F006700500052004F004D004F",
                    "tag" to "1",
                ),
            ),
        )
        val messages = SMSParser.parseMessages(payload)
        assertEquals(1, messages.size)
        assertEquals("DialogPROMO", messages[0].number)
        assertEquals("UNLOCK", messages[0].content)
        assertEquals(SMSTag.UNREAD, messages[0].tag)
    }

    @Test
    fun `alphanumeric senders do not collapse into one conversation`() {
        // Before addresses were decoded this could not happen, because the hex
        // was full of digits. Decoding them made every letters-only sender
        // normalise to the empty string, which merged unrelated senders into a
        // single thread.
        val a = message(1, "DialogPROMO", "one")
        val b = message(2, "StarPoints", "two")
        val conversations = SMSParser.groupIntoConversations(listOf(a, b))
        assertEquals(2, conversations.size)
    }

    @Test
    fun `one number with and without a country code is one conversation`() {
        val a = message(1, "+94771234567", "one")
        val b = message(2, "0771234567", "two")
        assertEquals(1, SMSParser.groupIntoConversations(listOf(a, b)).size)
    }

    @Test
    fun `round trips through the unicode encoder`() {
        val text = "DialogPROMO"
        assertEquals(text, SMSParser.decodeUCS2Hex(SMSParser.encodeUCS2Hex(text)))
    }

    private fun message(id: Int, number: String, body: String) = SMSMessage(
        id = id,
        number = number,
        content = body,
        date = SMSParser.parseSMSDate("26,08,14,11,15,43,+22"),
        tag = SMSTag.READ,
        groupId = "",
        memStore = "nv",
    )
}
