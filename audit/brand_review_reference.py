#!/usr/bin/env python3
"""Offline successor-giveaway reference: exact native hashes and independent replay.

This module performs no RPC, signing, deployment, or external-data verification.
Full sorting deliberately differs from the contracts' bounded ranking algorithms.
Input is a complete normalized offline history. Authenticating transactions,
finality, and the campaign/taskbook/entries/selection emitters is a separate gate.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass, field
import hashlib
import json
from pathlib import Path
from typing import Any

ZERO = bytes(32)
WHITESPACE = b" \t\n\r\v\f"
LIMITS = {"tasks": 32, "title": 64, "instructions": 256, "target_uri": 128,
          "evidence_prompt": 128, "evidence": 256, "versions": 4,
          "current_evidence": 8192, "participants": 4096, "winners": 1000}


class Invalid(ValueError):
    """History contradicts the frozen protocol."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise Invalid(message)


def sha(data: bytes) -> bytes:
    return hashlib.sha256(data).digest()


def uint(value: int, size: int, endian: str = "little") -> bytes:
    require(type(value) is int and 0 <= value < 1 << (8 * size), "integer out of range")
    return value.to_bytes(size, endian)


def text_bytes(value: str, maximum: int, *, allow_empty: bool = False) -> bytes:
    require(type(value) is str, "text must be a string")
    try:
        data = value.encode("utf-8", "strict")
    except UnicodeError as exc:
        raise Invalid("invalid UTF-8 text") from exc
    require(len(data) <= maximum, "text exceeds byte limit")
    require(b"\0" not in data, "NUL is not permitted in text")
    require((allow_empty and not data) or bool(data.strip(WHITESPACE)),
            "empty or ASCII-whitespace-only text")
    return data


def address(value: str, size: int) -> bytes:
    require(type(value) is str, "address must be hexadecimal text")
    try:
        raw = bytes.fromhex(value.removeprefix("0x"))
    except ValueError as exc:
        raise Invalid("invalid address") from exc
    require(len(raw) == size, "wrong address width")
    return raw


def blob(value: bytes) -> bytes:
    return uint(len(value), 4) + value


def abi(*values: int | bytes | str | bool) -> bytes:
    """Independent standard ABI encoding for uint/address, bytes32 and strings."""
    head: list[bytes] = []
    tail = b""
    for value in values:
        if type(value) is str:
            data = value.encode("utf-8", "strict")
            head.append(uint(32 * len(values) + len(tail), 32, "big"))
            tail += uint(len(data), 32, "big") + data + bytes((-len(data)) % 32)
        elif type(value) is bytes:
            require(len(value) <= 32, "ABI word too large")
            head.append(value.rjust(32, b"\0"))
        else:
            head.append(uint(int(value) if type(value) is bool else value, 32, "big"))
    return b"".join(head) + tail


@dataclass(frozen=True)
class Task:
    task_id: int
    title: str
    instructions: str
    target_uri: str
    evidence_prompt: str
    max_evidence_bytes: int = 256

    def validate(self) -> None:
        require(0 <= self.task_id < LIMITS["tasks"], "task id out of range")
        for name in ("title", "instructions", "target_uri", "evidence_prompt"):
            text_bytes(getattr(self, name), LIMITS[name], allow_empty=name == "target_uri")
        require(1 <= self.max_evidence_bytes <= LIMITS["evidence"], "invalid evidence limit")


class HashProfile:
    def __init__(self, family: str, instance: str, chain_id: int = 1, giveaway_id: int = 1):
        require(family in ("solana", "evm"), "unknown hash profile")
        self.family, self.chain_id, self.giveaway_id = family, chain_id, giveaway_id
        self.width = 32 if family == "solana" else 20
        self.instance = address(instance, self.width)

    def wallet(self, value: str) -> bytes:
        raw = address(value, self.width)
        canonical = ("0x" if self.family == "evm" else "") + raw.hex()
        require(value == canonical, "address is not canonical lowercase hexadecimal")
        return raw

    @staticmethod
    def domain(label: str) -> bytes:
        return sha(("POC_GIVEAWAY_EVM_V2_" + label).encode("ascii"))

    def task_hash(self, task: Task) -> bytes:
        task.validate()
        if self.family == "evm":
            return sha(abi(self.domain("TASK"), task.task_id, task.title, task.instructions,
                           task.target_uri, task.evidence_prompt, task.max_evidence_bytes))
        record = uint(task.task_id, 2)
        for value in (task.title, task.instructions, task.target_uri, task.evidence_prompt):
            record += blob(value.encode("utf-8"))
        return sha(b"GIVEAWAY_TASK_V2" + record + uint(task.max_evidence_bytes, 2))

    def task_root(self, tasks: list[Task]) -> bytes:
        root = ZERO if self.family == "solana" else self.domain("TASKS")
        for expected, task in enumerate(tasks):
            require(task.task_id == expected, "tasks are not dense and ordered")
            leaf = self.task_hash(task)
            root = (sha(b"GIVEAWAY_TASKS_V2" + root + leaf) if self.family == "solana"
                    else sha(abi(root, task.task_id, leaf)))
        return root

    def evidence_hash(self, wallet: str, task_id: int, version: int, text: str) -> bytes:
        data = text_bytes(text, LIMITS["evidence"])
        owner = self.wallet(wallet)
        if self.family == "evm":
            return sha(abi(self.domain("EVIDENCE"), self.chain_id, self.instance,
                           owner, task_id, version, text))
        return sha(b"GIVEAWAY_EVIDENCE_V2" + self.instance + owner + uint(task_id, 2)
                   + uint(version, 1) + blob(data))

    def entry_root(self, wallet: str, evidence: list[tuple[int, str]],
                   revision: int, commitment: bytes) -> bytes:
        root = ZERO if self.family == "solana" else self.domain("ENTRY")
        for task_id, (version, text) in enumerate(evidence):
            leaf = self.evidence_hash(wallet, task_id, version, text)
            root = (sha(b"GIVEAWAY_ENTRY_V2" + root + leaf) if self.family == "solana"
                    else sha(abi(root, task_id, leaf)))
        return root if self.family == "solana" else sha(abi(root, revision, commitment))

    def phrase_digest(self, wallet: str, phrase: bytes) -> bytes:
        require(1 <= len(phrase) <= 32 and b"\x1f" not in phrase, "invalid lucky phrase")
        if self.family == "solana":
            return sha(b"GIVEAWAY_PHRASE_V2" + self.wallet(wallet) + blob(phrase))
        return sha(abi(self.domain("PHRASE"), self.chain_id, self.instance,
                       self.giveaway_id, self.wallet(wallet), sha(phrase)))

    def pool_and_seed(self, registered: list["Entry"], winners: int) -> tuple[bytes, bytes]:
        pool = ZERO if self.family == "solana" else self.domain("POOL")
        aggregate, count = 0, 0
        for entry in registered:
            require(entry.index is not None, "pool contains unregistered entry")
            included = entry.sealed and entry.digest is not None
            digest = entry.digest if included else ZERO
            if included:
                count += 1
                aggregate ^= int.from_bytes(digest, "big")
            if self.family == "solana":
                if included:
                    pool = sha(b"GIVEAWAY_POOL_V2" + pool + uint(entry.index, 8)
                               + self.wallet(entry.wallet) + digest)
            else:
                pool = sha(abi(pool, entry.index, self.wallet(entry.wallet), digest, included))
        aggregate_bytes = aggregate.to_bytes(32, "big")
        if self.family == "solana":
            seed = sha(b"GIVEAWAY_SEED_V2" + self.instance + uint(count, 8) + aggregate_bytes + pool)
        else:
            seed = sha(abi(self.domain("SEED"), self.chain_id, self.instance, self.giveaway_id,
                           aggregate_bytes, pool, count, winners))
        return pool, seed

    def candidate_key(self, seed: bytes, wallet: str) -> bytes:
        owner = self.wallet(wallet)
        score = (sha(b"GIVEAWAY_RANK_V2" + seed + owner) if self.family == "solana"
                 else sha(abi(self.domain("CANDIDATE"), seed, owner)))
        return score + owner

    def winner_leaf(self, slot: int, wallet: str, amount: int, evidence_root: bytes,
                    revision: int, ordinal: int) -> bytes:
        require(self.family == "evm", "Solana pays exact accepted-slot accounts")
        inner = sha(abi(self.domain("WINNER"), self.chain_id, self.instance, self.giveaway_id,
                        slot, self.wallet(wallet), amount, evidence_root, revision, ordinal))
        return sha(inner)

    def accepted_root(self, winners: list[dict[str, Any]]) -> bytes:
        require(self.family == "solana", "EVM uses ordered winner Merkle leaves")
        root = ZERO
        for slot, winner in enumerate(winners):
            root = sha(b"GIVEAWAY_ACCEPTED_V2" + root + uint(slot, 4)
                       + uint(winner["ordinal"], 8) + self.wallet(winner["wallet"])
                       + bytes.fromhex(winner["evidence_root"]) + uint(winner["revision"], 4))
        return root

    def outcome_hash(self, rules_hash: bytes, seed: bytes, winner_root: bytes, count: int,
                     payout: int, fee: int, remainder: int, consumed: int) -> bytes:
        if self.family == "solana":
            return sha(b"GIVEAWAY_OUTCOME_V2" + self.instance + rules_hash + winner_root
                       + uint(count, 4) + uint(payout, 8) + uint(fee, 8) + uint(remainder, 8))
        return sha(abi(self.domain("OUTCOME"), rules_hash, seed, winner_root, count,
                       payout, fee, remainder, consumed))


def merkle_root(leaves: list[bytes]) -> bytes:
    require(bool(leaves) and all(len(leaf) == 32 for leaf in leaves), "invalid winner leaves")
    nodes = list(leaves) + [ZERO] * ((1 << (len(leaves) - 1).bit_length()) - len(leaves))
    while len(nodes) > 1:
        nodes = [sha(nodes[i] + nodes[i + 1]) for i in range(0, len(nodes), 2)]
    return nodes[0]


@dataclass
class Entry:
    wallet: str
    commitment: bytes = ZERO
    items: dict[int, list[str]] = field(default_factory=dict)
    revision: int = 0
    mutation: int = 0
    sealed: bool = False
    root: bytes | None = None
    index: int | None = None
    seal_revision: int | None = None
    seal_cursor: int = 0
    attested: bool = False
    digest: bytes | None = None


@dataclass(frozen=True)
class Terms:
    brand: str
    host: str
    funding: int = 10003
    fee_bps: int = 500
    winners: int = 2
    participant_cap: int = 4096
    start: int = 100
    entry_end: int = 200
    reveal_end: int = 300
    remediation_end: int = 400
    opening_cutoff: int = 500
    review_duration: int = 100
    final_deadline: int = 650
    finalization_reserve: int = 50

    def validate(self) -> None:
        require(0 <= self.fee_bps < 10000, "invalid fee")
        require(1 <= self.winners <= LIMITS["winners"], "invalid winner count")
        require(1 <= self.participant_cap <= LIMITS["participants"], "invalid participant cap")
        require(self.funding - self.funding * self.fee_bps // 10000 >= self.winners,
                "funding cannot support minimum payouts")
        require(self.start < self.entry_end < self.reveal_end <= self.remediation_end
                < self.opening_cutoff and self.review_duration > 0 and self.finalization_reserve > 0
                and self.opening_cutoff + self.review_duration + self.finalization_reserve
                <= self.final_deadline, "invalid frozen schedule")


class Campaign:
    def __init__(self, profile: HashProfile, tasks: list[Task], terms: Terms):
        terms.validate()
        require(1 <= len(tasks) <= LIMITS["tasks"], "invalid task count")
        self.profile, self.tasks, self.terms = profile, tuple(tasks), terms
        profile.wallet(terms.brand)
        profile.wallet(terms.host)
        self.task_root = profile.task_root(tasks)
        self.entries: dict[str, Entry] = {}
        self.registered: list[str] = []
        self.pool: bytes | None = None
        self.seed: bytes | None = None
        self.ranking: list[str] | None = None
        self.pending: dict[str, Any] | None = None
        self.decisions: list[dict[str, Any]] = []
        self.approved: list[str] = []
        self.outcome: dict[str, Any] | None = None
        self.preparing = False
        self.paid: set[str] = set()

    def active(self, now: int) -> None:
        require(self.terms.start <= now < self.terms.entry_end and self.outcome is None,
                "entry window closed")

    def host(self, signer: str) -> None:
        require(signer == self.terms.host, "host signature required")

    def publish(self, wallet: str, task_id: int, text: str, now: int) -> None:
        self.active(now)
        self.profile.wallet(wallet)
        require(0 <= task_id < len(self.tasks), "unknown task")
        data = text_bytes(text, self.tasks[task_id].max_evidence_bytes)
        entry = self.entries.setdefault(wallet, Entry(wallet))
        versions = entry.items.get(task_id, [])
        require(len(versions) < LIMITS["versions"], "evidence revision cap")
        total = sum(len(items[-1].encode("utf-8")) for key, items in entry.items.items()
                    if key != task_id) + len(data)
        require(total <= LIMITS["current_evidence"], "total evidence byte limit")
        entry.items.setdefault(task_id, []).append(text)
        entry.mutation += 1
        if self.profile.family == "solana":
            entry.revision += 1
        entry.sealed, entry.root = False, None

    def commit(self, wallet: str, commitment: str, now: int) -> None:
        self.active(now)
        self.profile.wallet(wallet)
        raw = address(commitment, 32)
        require(raw != ZERO, "zero phrase commitment")
        entry = self.entries.setdefault(wallet, Entry(wallet))
        entry.commitment = raw
        entry.mutation += 1
        if self.profile.family == "solana":
            entry.revision += 1
        entry.sealed, entry.root = False, None

    def begin_seal(self, wallet: str, now: int) -> None:
        self.active(now)
        entry = self.entries[wallet]
        require(entry.commitment != ZERO, "missing phrase commitment")
        if self.profile.family == "solana":
            require(not entry.sealed, "entry is already sealed")
        entry.seal_revision, entry.seal_cursor = entry.mutation, 0

    def process_seal(self, wallet: str, task_ids: list[int], now: int) -> None:
        self.active(now)
        entry = self.entries[wallet]
        require(entry.seal_revision == entry.mutation, "stale seal session")
        expected = list(range(entry.seal_cursor, entry.seal_cursor + len(task_ids)))
        require(bool(task_ids) and all(type(i) is int for i in task_ids)
                and task_ids == expected and expected[-1] < len(self.tasks),
                "seal coverage mismatch")
        require(all(task_id in entry.items for task_id in task_ids), "missing required evidence")
        entry.seal_cursor += len(task_ids)

    def finish_seal(self, wallet: str, now: int) -> None:
        self.active(now)
        entry = self.entries[wallet]
        require(entry.seal_revision == entry.mutation and entry.seal_cursor == len(self.tasks),
                "incomplete or stale seal")
        if self.profile.family == "evm":
            require(entry.revision < 128, "entry revision cap")
            entry.revision += 1
        if entry.index is None:
            require(len(self.registered) < self.terms.participant_cap, "participant cap")
            entry.index = len(self.registered)
            self.registered.append(wallet)
        evidence = [(len(entry.items[i]), entry.items[i][-1]) for i in range(len(self.tasks))]
        entry.root = self.profile.entry_root(wallet, evidence, entry.revision, entry.commitment)
        entry.sealed = True

    def attest(self, wallet: str, now: int) -> None:
        require(self.terms.entry_end <= now < self.terms.reveal_end and self.seed is None,
                "attestation window closed")
        entry = self.entries[wallet]
        require(entry.sealed and not entry.attested, "entry cannot attest")
        entry.attested = True

    def reveal(self, wallet: str, phrase: str, salt_hex: str, now: int,
               signer: str | None = None) -> None:
        require(self.terms.entry_end <= now < self.terms.remediation_end and self.seed is None,
                "reveal window closed")
        entry = self.entries[wallet]
        require(entry.sealed and entry.digest is None, "entry cannot reveal")
        if signer is None:
            require(now < self.terms.reveal_end, "direct attestation window closed")
        else:
            require(entry.attested, "host reveal lacks accepted attestation")
            if now < self.terms.reveal_end:
                self.host(signer)
                require(all(self.entries[w].attested for w in self.registered
                            if self.entries[w].sealed), "early host publication")
        phrase_bytes = text_bytes(phrase, 32)
        try:
            salt = bytes.fromhex(salt_hex)
        except ValueError as exc:
            raise Invalid("invalid salt") from exc
        require(len(salt) <= 16, "invalid salt length")
        require(sha(phrase_bytes + b"\x1f" + salt) == entry.commitment, "commitment mismatch")
        entry.digest = self.profile.phrase_digest(wallet, phrase_bytes)
        entry.attested = True

    def freeze(self, signer: str, now: int) -> None:
        self.host(signer)
        require(self.seed is None and self.outcome is None and now < self.terms.final_deadline,
                "pool already frozen or campaign closed")
        participants = [self.entries[w] for w in self.registered if self.entries[w].sealed]
        require(now >= self.terms.reveal_end or
                (now >= self.terms.entry_end and participants and all(e.attested for e in participants)),
                "reveal window still open")
        require(all(not e.attested or e.digest is not None for e in participants),
                "missing accepted reveal")
        included = [e for e in participants if e.digest is not None]
        self.pool, self.seed = self.profile.pool_and_seed([self.entries[w] for w in self.registered],
                                                         self.terms.winners)
        self.ranking = sorted((e.wallet for e in included),
                              key=lambda wallet: self.profile.candidate_key(self.seed, wallet))

    def open_candidate(self, signer: str, now: int) -> dict[str, Any]:
        self.host(signer)
        require(self.ranking is not None and self.pending is None and self.outcome is None
                and not self.preparing, "candidate cannot open")
        require(now < self.terms.opening_cutoff, "candidate opening cutoff")
        require(len(self.approved) < self.terms.winners and len(self.decisions) < len(self.ranking),
                "selection terminal")
        ordinal = len(self.decisions)
        wallet = self.ranking[ordinal]
        entry = self.entries[wallet]
        self.pending = {"ordinal": ordinal, "round": ordinal + 1, "wallet": wallet,
                        "slot": len(self.approved), "entry_revision": entry.revision,
                        "evidence_root": entry.root.hex(), "opened_at": now,
                        "review_deadline": now + self.terms.review_duration}
        return dict(self.pending)

    def review(self, signer: str, ordinal: int, approve: bool, now: int,
               *, reason: int = 0, note: str = "", task_ids: list[int] | None = None) -> None:
        require(signer == self.terms.brand, "brand signature required")
        require(self.pending is not None and self.outcome is None and not self.preparing,
                "no pending review")
        require(ordinal == self.pending["ordinal"] and now < self.pending["review_deadline"],
                "stale candidate or expired review")
        require(type(approve) is bool and type(reason) is int and ((approve and reason == 0) or
                (not approve and 1 <= reason <= 3)), "invalid brand decision")
        text_bytes(note, 128 if self.profile.family == "solana" else 256, allow_empty=True)
        refs = task_ids or []
        require(len(set(refs)) == len(refs) and all(0 <= i < len(self.tasks) for i in refs),
                "invalid task references")
        decision = dict(self.pending, approve=approve, reason=reason, note=note,
                        task_ids=refs, decided_at=now)
        if approve:
            self.approved.append(self.pending["wallet"])
        self.decisions.append(decision)
        self.pending = None

    def prepare_outcome(self, signer: str, now: int) -> None:
        self.host(signer)
        require(self.ranking is not None and self.pending is None and self.outcome is None
                and now < self.terms.final_deadline, "outcome cannot prepare")
        require(len(self.approved) == self.terms.winners or len(self.decisions) == len(self.ranking),
                "pool not fully consumed")
        require(bool(self.approved), "zero approvals require exhaustion refund")
        self.preparing = True

    def lock(self, signer: str, now: int) -> None:
        self.host(signer)
        require(self.preparing and self.outcome is None and now < self.terms.final_deadline,
                "outcome cannot lock")
        fee = self.terms.funding * self.terms.fee_bps // 10000
        payout = (self.terms.funding - fee) // len(self.approved)
        self.outcome = {"kind": "success", "winners": list(self.approved), "fee": fee,
                        "per_winner": payout,
                        "creator_refund": self.terms.funding - fee - payout * len(self.approved)}

    def refund(self, now: int) -> None:
        require(self.outcome is None, "vested or refunded campaign cannot refund")
        reason = None
        if now >= self.terms.final_deadline:
            reason = "absolute_expiry"
        elif self.pending and now >= self.pending["review_deadline"]:
            reason = "review_expiry"
        elif self.pending is None and self.ranking is not None:
            if len(self.decisions) == len(self.ranking) and not self.approved:
                reason = "zero_approved_exhaustion"
            elif now >= self.terms.opening_cutoff and len(self.approved) < self.terms.winners:
                if len(self.decisions) < len(self.ranking):
                    reason = "opening_cutoff_unresolved"
        elif now >= self.terms.remediation_end:
            if any(e.attested and e.digest is None for e in self.entries.values()):
                reason = "missing_accepted_reveal"
        require(reason is not None, "refund not available")
        self.outcome = {"kind": "refund", "reason": reason, "winners": [], "fee": 0,
                        "per_winner": 0, "creator_refund": self.terms.funding}

    def pay(self, wallet: str) -> int:
        require(self.outcome is not None and self.outcome["kind"] == "success"
                and wallet in self.outcome["winners"] and wallet not in self.paid,
                "no unpaid vested entitlement")
        self.paid.add(wallet)
        return self.outcome["per_winner"]

    def snapshot(self) -> dict[str, Any]:
        return {"task_root": self.task_root.hex(), "registered": list(self.registered),
                "pool": self.pool.hex() if self.pool else None,
                "seed": self.seed.hex() if self.seed else None, "ranking": self.ranking,
                "pending": self.pending, "decisions": self.decisions,
                "approved": list(self.approved), "outcome": self.outcome, "paid": sorted(self.paid)}


def replay(document: dict[str, Any]) -> dict[str, Any]:
    profile = HashProfile(**document["profile"])
    campaign = Campaign(profile, [Task(**task) for task in document["tasks"]], Terms(**document["terms"]))
    last_time = -1
    for position, event in enumerate(document["events"]):
        operation = event.get("operation")
        require(operation in {"publish", "commit", "begin_seal", "process_seal", "finish_seal",
                              "attest", "reveal", "freeze", "open_candidate", "review",
                              "prepare_outcome", "lock", "refund", "pay"}, "unknown operation")
        try:
            now = event.get("now", last_time)
            require(type(now) is int and now >= last_time, "history time moved backwards")
            last_time = now
            observed = event.get("observed")
            result = getattr(campaign, operation)(**{k: v for k, v in event.items()
                                                    if k not in ("operation", "observed")})
            if observed is not None:
                require(result == observed, "observed candidate or payout differs")
        except (Invalid, KeyError, TypeError) as exc:
            raise Invalid(f"event {position} ({operation}): {exc}") from exc
    snapshot = campaign.snapshot()
    if "claimed_snapshot" in document:
        require(snapshot == document["claimed_snapshot"], "claimed snapshot differs")
    if "native_counters" in document:
        # EVM consumes a heap rank at opening; Solana counts terminal verdicts.
        # These are equal only when no candidate remains pending.
        terminal = len(campaign.decisions)
        opened = terminal + int(campaign.pending is not None)
        counters = {"registered": len(campaign.registered), "approved": len(campaign.approved),
                    "opened": opened, "terminal": terminal,
                    "native_consumed": opened if profile.family == "evm" else terminal}
        require(counters == document["native_counters"], "native counters differ")
    return snapshot


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("history", type=Path, help="offline JSON history, not an RPC URL")
    args = parser.parse_args()
    try:
        result = replay(json.loads(args.history.read_text()))
    except (Invalid, ValueError, KeyError, TypeError) as exc:
        print(json.dumps({"status": "contradictory", "error": str(exc)}))
        return 1
    print(json.dumps({"status": "reference_consistent", "result": result}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
