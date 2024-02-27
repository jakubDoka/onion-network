
/**
 * @type {IDBDatabase}
 * @global
 */
let __db;

/**
 * @returns {Promise<IDBDatabase>}
 * @async
 */
function openDb() {
        const dbReq = window.indexedDB.open("appCache", 2);
        return new Promise((resolve, reject) => {
                if (__db) return resolve(__db);
                dbReq.onsuccess = (event) => (__db = event.target.result, resolve(__db));
                dbReq.onerror = (event) => reject(event.target.error);
                dbReq.onupgradeneeded = (event) => {
                        /** @type {IDBDatabase} */
                        const ndb = event.target.result;
                        ndb.deleteObjectStore("messages");
                        const messages = ndb.createObjectStore("messages", { autoIncrement: true });
                        messages.createIndex("by_chat", ["chat", "owner"], { unique: false });
                };
        });
}

/**
 * @typedef {Object} Message
 * @property {string} chat
 * @property {string} author
 * @property {string} message
 */

/**
 * @param {string} json_messages
 */
async function saveMessagesDb(json_messages) {
        /** @type {Message[]} */
        const messages = JSON.parse(json_messages);

        const db = await openDb();
        const tx = db.transaction("messages", "readwrite");
        const store = tx.objectStore("messages");
        for (const message of messages) {
                store.put(message);
        }
        await tx.complete;
}

class MessageCursor {
        /**
         * @param {IDBRequest} req
         */
        #position = 0;
        #chat;
        #owner;

        constructor(chat, user) {
                this.#chat = chat;
                this.#owner = user;
        }

        /**
         * @param {number} amount
         * @returns {Promise<string>}
         */
        async next(amount) {
                const db = await openDb();
                const tx = db.transaction("messages", "readonly");
                const store = tx.objectStore("messages");
                const index = store.index("by_chat");
                const req = index.openCursor([this.#chat, this.#owner], "prev");

                let advanced = this.#position === 0;
                let messages = [];
                return new Promise((resolve, reject) => {
                        req.onsuccess = (event) => {
                                /** @type {IDBCursorWithValue} */
                                const cursor = event.target.result;
                                if (cursor) {
                                        if (!advanced) {
                                                cursor.advance(this.#position);
                                                advanced = true;
                                        } else if (messages.length < amount) {
                                                this.#position++;
                                                messages.push(cursor.value);
                                                cursor.continue();
                                        } else {
                                                resolve(JSON.stringify(messages));
                                        }
                                } else {
                                        resolve(JSON.stringify(messages));
                                }
                        };
                        req.onerror = (event) => reject(event.target.error);
                });
        }
}

this.db = {
        saveMessages: saveMessagesDb,
};
