# Phrase X — configurare "Accedi con Google" (una volta, ~15 minuti)

Serve **solo a chi compila Phrase X** (chi installa l'app non deve fare nulla). Serve un client OAuth Google che Phrase X usa per salvare la libreria nello spazio nascosto
dell'app su Google Drive (appDataFolder). Lo crei una volta sola; vale per tutti i PC.

1. **Progetto** — apri https://console.cloud.google.com, in alto "Seleziona progetto" → **Nuovo progetto**,
   nome `Phrase X` → Crea, poi selezionalo.
2. **API Drive** — menu ☰ → *API e servizi* → *Libreria* → cerca **Google Drive API** → **Abilita**.
3. **Schermata di consenso** — menu ☰ → *Google Auth Platform* (o *Schermata consenso OAuth*) → **Inizia**:
   - Nome app `Phrase X`, email di assistenza = la tua.
   - Pubblico: **Interno** se l'account è Google Workspace aziendale (accedono solo gli utenti dell'organizzazione,
     nessuna scadenza); altrimenti **Esterno**.
   - Email di contatto → accetta → **Crea**.
4. **Ambiti** — *Accesso ai dati* → **Aggiungi o rimuovi ambiti**, spunta:
   `openid`, `.../auth/userinfo.email`, `.../auth/drive.appdata` → Aggiorna → **Salva**.
   (Sono ambiti non sensibili: niente verifica Google.)
5. **Solo se Esterno** — *Pubblico* → **Pubblica app** → Conferma.
   Se resta "In test", il login scade ogni 7 giorni (e devi aggiungere la tua email tra gli utenti di test).
6. **Client** — *Client* → **Crea client** → Tipo di applicazione **App desktop**, nome `Phrase X desktop` → Crea
   → nella finestra che compare **Scarica JSON**.
7. **Nel progetto** — salva il file scaricato, senza modificarlo, come:

   `app\src-tauri\google-oauth.json` (dentro la cartella del progetto)

   Il build legge `client_id` e `client_secret` da lì e li incorpora nell'exe (per le app desktop Google
   non considera segreto il client secret; la sicurezza viene da PKCE). Il file è in `.gitignore`.
   Senza il file Phrase X compila lo stesso, ma in Impostazioni il pulsante Google dice "client non configurato".
8. Ricompila e reinstalla (`scripts\reinstall.cmd`), poi Impostazioni → **Accedi con Google**.
