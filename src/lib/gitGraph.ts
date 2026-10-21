// The history graph of the Files window's Source control tab: each commit's row as VS Code draws
// it, its node on a lane and the lines that pass, meet it or leave it towards its parents.

export type GraphCommit = { id: string; parents: string[] };
/** A line of a row's upper or lower half: from lane `from` at its top to lane `to` at its bottom. */
export type Segment = { from: number; to: number; colour: number };
export type GraphRow = {
  /** The node's lane and colour. */
  lane: number;
  colour: number;
  /** Lines from the row's top edge to the node (`to` is the node's lane). */
  into: Segment[];
  /** Lines from the node to the row's bottom edge, one per parent. */
  out: Segment[];
  /** Lanes that pass the row untouched. */
  pass: Array<{ lane: number; colour: number }>;
  /** Lanes the row is wide. */
  width: number;
};

/** Colours a lane can take, used in turn as lanes start. */
export const COLOURS = 5;

/** The rows for `commits`, in the order given (`git log --topo-order`). */
export function layout(commits: GraphCommit[]): GraphRow[] {
  let lanes: Array<{ id: string; colour: number } | null> = [];
  let next = 0;
  const free = (): number => {
    const at = lanes.indexOf(null);
    return at < 0 ? lanes.length : at;
  };

  return commits.map((commit) => {
    let lane = lanes.findIndex((held) => held?.id === commit.id);
    const into: Segment[] = [];
    const pass: Array<{ lane: number; colour: number }> = [];
    let colour: number;

    if (lane < 0) {
      // A tip nothing above waited for: a lane of its own, nothing coming in.
      lane = free();
      colour = next++ % COLOURS;
      lanes[lane] = { id: commit.id, colour };
    } else {
      colour = lanes[lane]!.colour;
      lanes.forEach((held, at) => {
        if (held?.id === commit.id) into.push({ from: at, to: lane, colour: held.colour });
      });
    }
    lanes.forEach((held, at) => {
      if (held && held.id !== commit.id) pass.push({ lane: at, colour: held.colour });
    });

    lanes = lanes.map((held) => (held?.id === commit.id ? null : held));
    const out: Segment[] = [];
    commit.parents.forEach((parent, i) => {
      const waiting = lanes.findIndex((held) => held?.id === parent);
      if (waiting >= 0) {
        out.push({ from: lane, to: waiting, colour: lanes[waiting]!.colour });
      } else if (i === 0 && lanes[lane] === null) {
        lanes[lane] = { id: parent, colour };
        out.push({ from: lane, to: lane, colour });
      } else {
        const at = free();
        const fresh = next++ % COLOURS;
        lanes[at] = { id: parent, colour: fresh };
        out.push({ from: lane, to: at, colour: fresh });
      }
    });

    while (lanes.length && lanes[lanes.length - 1] === null) lanes.pop();
    const used = [lane + 1, lanes.length, ...into.map((s) => s.from + 1), ...out.map((s) => s.to + 1), ...pass.map((p) => p.lane + 1)];
    return { lane, colour, into, out, pass, width: Math.max(...used) };
  });
}
