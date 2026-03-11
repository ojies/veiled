# Veiled: Anonymous Bitcoin Payment Verification

Veiled is a privacy-preserving Bitcoin payment verification system. It allows members of a group to verify each other's legitimacy before making payments, without revealing their individual identities or linking their activities across different transactions.

---

## 🔒 The Private Payment Vault: An Analogy

Imagine a private payment vault shared by **1,024 friends**. 

Each friend contributes a **"locked box"** containing their secret identity. When a bill arrives, you don't reveal who you are; instead, you use a special **"bill-specific coupon"** generated from your private key to pay it anonymously.

To make this work without revealing your identity, you perform a digital magic trick: you mathematically **subtract your coupon from every single locked box** in the entire vault. This results in exactly **"zero"** only for your specific box, while everyone else's remains a random mess.

You then use a **"tree-shaped" shortcut** to prove to the group’s accountant that a "zero" exists *somewhere* in that pile of 1,024 results, without ever pointing to your own box. This ensures the group knows the payment is valid and authorized by a real member.

Because the accountant records your specific coupon, you can’t pay the same bill twice. And since your coupon for dinner looks totally different from your coupon for drinks, no one can track your spending habits, keeping your financial life private while staying within the group.

---

## 🛠️ How it Works: The Protocol Phases

### Phase 0: System Setup (Establishing the Group)
The system is initialized with a **Common Reference String (CRS)**. This is a set of public mathematical parameters (generators `g, h_1..h_L`) that everyone in the group agrees upon. These generators are "Nothing Up My Sleeve" (NUMS) points, meaning they are provably random and independent.

### Phase 1: Identity Creation (Local & Private)
Each user generates three secret values locally on their device:
1.  **Master Secret (`sk`):** Used to derive unique tokens (nullifiers) for each "bill" or group interaction.
2.  **Child Randomness (`r`):** Used to derive authentication keys.
3.  **Blinding Key (`k`):** A mathematical "shroud" that hides your identity within the group's commitments.

From these, you compute your **Master Identity (`Φ`)**, which is a multi-value Pedersen commitment. This single point (33 bytes) cryptographically "locks" all your potential tokens into one identity.

### Phase 2: Registration (Anchoring on Bitcoin)
You post your Master Identity `Φ` to a registry. Veiled uses **Bitcoin vtxo-trees**. 
- 1,024 users' identities are bundled into a single tree of pre-signed Bitcoin transactions.
- **Funding Membership:** To join the tree, each user contributes a specific amount of Bitcoin (decided by the registry). This value is reflected directly in your unique leaf output within the transaction tree.
- Only the **root** of this tree is broadcast on-chain, making it extremely efficient.
- Your identity `Φ` becomes a leaf in this tree, represented as a Pay-to-Taproot (P2TR) output.

Once the set of 1,024 is full, it is **sealed** (frozen forever). You now have an anonymity set.

### ⏳ Tree Lifetime & Timebound Verification
Every vtxo-tree has a defined **lifetime**. This means the payment verification is **timebound**.
- **Security against Highjacking:** If a user profile or device is compromised/hijacked, the damage is naturally limited by the tree's expiration. Once the tree's lifetime ends, the hijacked credentials can no longer be used for verification within that specific group context.
- **Renewal:** After a tree expires, legitimate users simply register into a new active tree to continue participating.

### Phase 3: Verification (The "Digital Magic Trick")
When you need to verify yourself to another group member:
1.  You derive a **Pseudonym** and a **Nullifier** for that specific interaction.
2.  You generate a **Bootle-Groth One-out-of-Many Proof**. This is the "magic trick" where you prove that subtracting your nullifier from the group's identities results in a "zero" at your (hidden) position.
3.  This proof is only **~2.4KB** and convinces the other member that you are a valid participant in the 1,024-person group without revealing which one.

### Phase 4: Payment & Authentication
Once verified, you can sign transactions or authorize payments using a **Child Credential** derived from your secret `r`. This is a standard Schnorr signature, which is lightning-fast and native to Bitcoin.

---

## 🛡️ Key Security Properties

- **Sybil Resistance:** You can only generate one valid nullifier per specific interaction. If you try to verify twice for the same "bill," the system will detect the duplicate nullifier and reject it.
- **Unlinkability:** Your identity for "User A" is mathematically independent of your identity for "User B." Even if they collaborate, they cannot prove you are the same person.
- **Bitcoin Native:** By using vtxo-trees and Taproot, Veiled anchors its security directly into the Bitcoin blockchain without requiring complex sidechains or high on-chain fees.
