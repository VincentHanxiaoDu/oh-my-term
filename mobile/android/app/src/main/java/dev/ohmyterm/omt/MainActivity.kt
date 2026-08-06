package dev.ohmyterm.omt

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp

/**
 * The app.
 *
 * The roster first, because a phone is picked up for ninety seconds because
 * something buzzed — and opening to a wall of terminal output has already
 * failed that.
 */
class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { MaterialTheme { Roster(emptyList(), "connecting", null) } }
    }
}

@Composable
fun Roster(rows: List<SessionRow>, connection: String, refusal: String?) {
    Column(Modifier.padding(16.dp)) {
        Text(
            rosterHeader(rows, connection, refusal),
            style = MaterialTheme.typography.headlineSmall,
        )
        LazyColumn {
            items(orderRoster(rows)) { row ->
                // The state is said in words, not carried by colour alone:
                // colour is invisible to a tenth of the people who use this,
                // and a screen reader gets nothing from it at all.
                val described = if (row.needsYou) "needs you" else row.state
                Text(
                    "${row.title} — $described",
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(vertical = 12.dp)
                        .semantics { contentDescription = "${row.title}, $described" },
                )
            }
        }
    }
}
