package com.openu60.core.model

/** One network from a manual operator scan. */
data class ScannedOperator(
    /** "available", "current", "forbidden" or "unknown". */
    val status: String,
    val name: String,
    val plmn: String,
    val mcc: String,
    val mnc: String,
    val rat: String,
    /** What operator select wants back to register on this network. */
    val select: String,
) {
    val isCurrent: Boolean get() = status == "current"
    val isForbidden: Boolean get() = status == "forbidden"
}

/** Result of GET /api/operator/scan. */
data class OperatorScan(
    val scanning: Boolean,
    /** The sweep ran and failed, as opposed to finding nothing. */
    val failed: Boolean,
    val operators: List<ScannedOperator>,
) {
    companion object {
        val empty = OperatorScan(scanning = false, failed = false, operators = emptyList())
    }
}

object OperatorParser {
    @Suppress("UNCHECKED_CAST")
    fun parseScan(data: Map<String, Any?>): OperatorScan {
        val list = data["operators"] as? List<*> ?: emptyList<Any?>()
        val operators = list.mapNotNull { item ->
            val o = item as? Map<String, Any?> ?: return@mapNotNull null
            ScannedOperator(
                status = o["status"]?.toString() ?: "unknown",
                name = o["name"]?.toString().orEmpty(),
                plmn = o["plmn"]?.toString().orEmpty(),
                mcc = o["mcc"]?.toString().orEmpty(),
                mnc = o["mnc"]?.toString().orEmpty(),
                rat = o["rat"]?.toString().orEmpty(),
                select = o["select"]?.toString().orEmpty(),
            )
        }
        return OperatorScan(
            scanning = data["scanning"] == true,
            failed = data["failed"] == true,
            operators = operators,
        )
    }
}
