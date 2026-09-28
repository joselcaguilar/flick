import { Badge, Button, GlassPanel, ListRow } from "../../components/ui";

const proItems = [
  {
    title: "Cloud setup assistant",
    body: "Opt-in single-frame help matching visible devices to Home Assistant entities.",
  },
  { title: "Multi-camera rooms", body: "Fuse anchors across room cameras for more resilient pointing." },
  { title: "Advanced camera sources", body: "RTSP and Unifi guidance arrive after the local-camera MVP." },
];

export function ProRoute() {
  return (
    <section className="feature-screen pro-screen" aria-labelledby="screen-title">
      <header className="operate-header">
        <div>
          <h1 id="screen-title">Core today. Pro later.</h1>
          <p>
            Flick stays local-first. Pro features will add optional setup assistance and larger-home
            workflows.
          </p>
        </div>
        <Badge tone="neutral">Not enabled</Badge>
      </header>
      <div className="pro-grid">
        <GlassPanel className="pro-hero-card">
          <Badge tone="accent">Phase 3</Badge>
          <h2>Keep the hot path on this Mac.</h2>
          <p>
            Gesture recognition, targeting, mappings and Home Assistant calls remain local. Pro only helps
            when you ask it to.
          </p>
          <Button variant="primary" disabled>
            License keys coming later
          </Button>
        </GlassPanel>
        <GlassPanel className="pro-list-card">
          {proItems.map((item) => (
            <ListRow key={item.title} title={item.title} description={item.body} trailing="Planned" />
          ))}
        </GlassPanel>
      </div>
    </section>
  );
}
