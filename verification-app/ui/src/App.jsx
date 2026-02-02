import React, { useState } from 'react';
import './index.css';

const API_BASE = 'http://localhost:3000';

function App() {
  const [results, setResults] = useState({});
  const [loading, setLoading] = useState({});

  const logResult = (feature, data) => {
    setResults(prev => ({ ...prev, [feature]: data }));
    setLoading(prev => ({ ...prev, [feature]: false }));
  };

  const startTest = (feature) => {
    setLoading(prev => ({ ...prev, [feature]: true }));
    setResults(prev => ({ ...prev, [feature]: null }));
  };

  const testCompression = async () => {
    startTest('compression');
    try {
      const res = await fetch(`${API_BASE}/compression/large`);
      const text = await res.text();
      const encoding = res.headers.get('content-encoding');
      logResult('compression', {
        result: 'Success',
        size: text.length,
        encoding: encoding || 'None (Check Network Tab)',
        details: `Received ${text.length} bytes.`
      });
    } catch (err) {
      logResult('compression', { result: 'Error', details: err.message });
    }
  };

  const testTimeout = async () => {
    startTest('timeout');
    const start = Date.now();
    try {
      const res = await fetch(`${API_BASE}/timeout/sleep`);
      if (res.status === 408 || res.status === 504) {
        logResult('timeout', {
          result: 'Success (Timed Out)',
          details: `Server returned ${res.status} as expected after ${Date.now() - start}ms.`
        });
      } else {
        logResult('timeout', { result: 'Failed', details: `Status: ${res.status}. Should be 408/504.` });
      }
    } catch (err) {
      const duration = Date.now() - start;
      logResult('timeout', {
        result: 'Success (Timed Out)',
        details: `Request failed as expected after ${duration}ms.`,
        error: err.message
      });
    }
  };

  const testAuth = async (token) => {
    startTest('auth');
    try {
      const headers = token ? { 'Authorization': `Bearer ${token}` } : {};
      const res = await fetch(`${API_BASE}/auth/protected`, { headers });
      if (res.ok) {
        const text = await res.text();
        logResult('auth', { result: 'Authorized', details: text, status: res.status });
      } else {
        logResult('auth', { result: 'Unauthorized', details: `Status: ${res.status}`, status: res.status });
      }
    } catch (err) {
      logResult('auth', { result: 'Error', details: err.message });
    }
  };

  const testLimit = async () => {
    startTest('limit');
    try {
      const largeBody = "A".repeat(2000); // Limit is 1024
      const res = await fetch(`${API_BASE}/limit/upload`, {
        method: 'POST',
        body: largeBody
      });
      if (res.ok) {
        logResult('limit', { result: 'Failed', details: 'Should have been rejected (413).' });
      } else {
        logResult('limit', { result: 'Success (Rejected)', details: `Status: ${res.status}`, status: res.status });
      }
    } catch (err) {
      logResult('limit', { result: 'Error', details: err.message });
    }
  };

  const testRequestId = async () => {
    startTest('requestId');
    try {
      const res = await fetch(`${API_BASE}/request-id`);
      const text = await res.text();
      const headerId = res.headers.get('x-request-id');
      logResult('requestId', {
        result: 'Success',
        body: text,
        header: headerId || 'Missing header'
      });
    } catch (err) {
      logResult('requestId', { result: 'Error', details: err.message });
    }
  };

  return (
    <div className="app">
      <h1>Tower HTTP Verification</h1>
      <p style={{ color: '#94a3b8', marginBottom: '3rem' }}>
        Interactive dashboard to verify middleware features.
      </p>

      <div className="grid-layout">
        <FeatureCard
          title="Compression"
          description="Fetches a large response. Expecting gzip/br encoding."
          onAction={testCompression}
          loading={loading['compression']}
          result={results['compression']}
        />

        <FeatureCard
          title="Timeout"
          description="Request sleeps for 5s. Server timeout is 2s."
          onAction={testTimeout}
          loading={loading['timeout']}
          result={results['timeout']}
        />

        <div className="glass-panel" style={{ padding: '2rem', textAlign: 'left' }}>
          <h3>Authorization</h3>
          <p style={{ fontSize: '0.9em', color: '#cbd5e1' }}>Values: Bearer secret-token</p>
          <div style={{ display: 'flex', gap: '1rem', marginTop: '1rem' }}>
            <button className="btn btn-secondary" onClick={() => testAuth(null)}>Test No Token</button>
            <button className="btn" onClick={() => testAuth('secret-token')}>Test Valid Token</button>
          </div>
          <ResultBox result={results['auth']} loading={loading['auth']} />
        </div>

        <FeatureCard
          title="Rate/Body Limit"
          description="Sends 2KB payload. Limit is 1KB."
          onAction={testLimit}
          loading={loading['limit']}
          result={results['limit']}
        />

        <FeatureCard
          title="Request ID"
          description="Checks generation and propagation of x-request-id."
          onAction={testRequestId}
          loading={loading['requestId']}
          result={results['requestId']}
        />
      </div>
    </div>
  );
}

function FeatureCard({ title, description, onAction, result, loading }) {
  return (
    <div className="glass-panel" style={{ padding: '2rem', textAlign: 'left', display: 'flex', flexDirection: 'column' }}>
      <h3>{title}</h3>
      <p style={{ fontSize: '0.9em', color: '#cbd5e1', flex: 1 }}>{description}</p>
      <button className="btn" onClick={onAction} disabled={loading} style={{ marginTop: '1rem', width: '100%' }}>
        {loading ? 'Testing...' : 'Run Test'}
      </button>
      <ResultBox result={result} loading={loading} />
    </div>
  );
}

function ResultBox({ result, loading }) {
  if (loading) return <div style={{ marginTop: '1rem', color: '#94a3b8' }}>Waiting for response...</div>;
  if (!result) return null;

  const isSuccess = result.result?.toLowerCase().includes('success') || result.result === 'Authorized';

  return (
    <div style={{ marginTop: '1rem', background: 'rgba(0,0,0,0.3)', padding: '1rem', borderRadius: '8px' }}>
      <div className={`status-badge ${isSuccess ? 'status-success' : 'status-error'}`}>
        {result.result}
      </div>
      <div style={{ marginTop: '0.5rem', fontSize: '0.85em', color: '#e2e8f0', wordBreak: 'break-all' }}>
        {Object.entries(result).map(([k, v]) => (
          k !== 'result' && <div key={k}><strong>{k}:</strong> {v}</div>
        ))}
      </div>
    </div>
  );
}

export default App;
