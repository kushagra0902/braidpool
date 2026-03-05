import { useState, useEffect, useRef } from 'react';
import { Link } from 'react-router-dom';
import { Miner, HistoryPoint, MinerAnalyticsPoint, Alert } from './Types';
import { API_URLS } from '../../URLs';
import AnalyticsCharts from './AnalyticsCharts';
import colors from '../../theme/colors';

const MAX_HISTORY_POINTS = 20;

const MinerInventoryDashboard = () => {
  const [miners, setMiners] = useState<Miner[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [newMinerIP, setNewMinerIP] = useState('');
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [searchInput, setSearchInput] = useState('');
  const [searchQuery, setSearchQuery] = useState('');
  const [sortBy, setSortBy] = useState<
    'all' | 'efficiency' | 'hashrate' | 'power' | 'temperature'
  >('all');
  const [statusFilter, setStatusFilter] = useState<
    'all' | 'online' | 'warning' | 'offline'
  >('all');
  const [refreshInterval, setRefreshInterval] = useState(30);
  const [lastUpdate, setLastUpdate] = useState<Date | null>(null);
  const minersRef = useRef<Miner[]>([]);
  const [fleetHistory, setFleetHistory] = useState<HistoryPoint[]>([]);
  const [minerHistory, setMinerHistory] = useState<
    Record<string, MinerAnalyticsPoint[]>
  >({});
  const [expandedAlerts, setExpandedAlerts] = useState<Record<string, boolean>>(
    {}
  );

  useEffect(() => {
    minersRef.current = miners;
  }, [miners]);

  useEffect(() => {
    if (miners.length === 0) return;
    const anyActive = miners.some(
      (miner) => miner.status === 'online' || miner.status === 'warning'
    );
    if (!anyActive) return;

    const timestamp = Date.now();
    const totalHashrateNow = miners.reduce(
      (sum, m) =>
        m.status === 'online' || m.status === 'warning'
          ? sum + (m.hashrate_current || 0)
          : sum,
      0
    );
    const totalExpectedNow = miners.reduce(
      (sum, m) =>
        m.status === 'online' || m.status === 'warning'
          ? sum + (m.expected_hashrate || 0)
          : sum,
      0
    );
    const activeMiners = miners.filter(
      (m) => m.status === 'online' || m.status === 'warning'
    );

    const avgEfficiencyNow =
      activeMiners.length > 0
        ? (activeMiners.reduce((sum, m) => sum + (m.efficiency || 0), 0) /
            activeMiners.length) *
          1000
        : 0;
    const avgTempNow =
      activeMiners.length > 0
        ? activeMiners.reduce((sum, m) => sum + (m.temperature || 0), 0) /
          activeMiners.length
        : 0;
    const avgVrTempNow =
      activeMiners.length > 0
        ? activeMiners.reduce((sum, m) => sum + (m.vr_temperature || 0), 0) /
          activeMiners.length
        : 0;

    setFleetHistory((prev) => {
      const next = [
        ...prev,
        {
          timestamp,
          totalHashrate: totalHashrateNow,
          expectedHashrate: totalExpectedNow,
          efficiency: avgEfficiencyNow,
          temperature: avgTempNow,
          vrTemperature: avgVrTempNow,
        },
      ];
      return next.slice(-MAX_HISTORY_POINTS);
    });

    setMinerHistory((prev) => {
      const next = { ...prev };
      miners.forEach((miner) => {
        const history = next[miner.id] ?? [];
        const point: MinerAnalyticsPoint = {
          timestamp,
          hashrate: miner.hashrate_current || 0,
          expected: miner.expected_hashrate || 0,
          efficiency: (miner.efficiency || 0) * 1000,
          temperature: miner.temperature || 0,
          vrTemperature: miner.vr_temperature || 0,
        };
        next[miner.id] = [...history, point].slice(-MAX_HISTORY_POINTS);
      });
      return next;
    });
  }, [miners]);

  // Helper functions
  const determineStatus = (data: any): 'online' | 'warning' | 'offline' => {
    if (!data.is_mining || (data.hashrate_current || 0) === 0) return 'offline';
    if (
      (data.temperature || 0) > 80 ||
      (data.vr_temperature || 0) > 85 ||
      data.errors?.length > 0
    )
      return 'warning';
    return 'online';
  };

  const getAlerts = (miner: Miner): Alert[] => {
    if (miner.status === 'offline') return [];
    const alerts: Alert[] = [];
    if (miner.hashrate_current <= 0) {
      alerts.push({ message: `Hashrate is zero`, severity: 'warning' });
    }
    if (miner.temperature > 100) {
      alerts.push({ message: `ASIC Temp High`, severity: 'critical' });
    }
    if (miner.vr_temperature > 105) {
      alerts.push({ message: `VR Temp High`, severity: 'critical' });
    }
    if (miner.voltage && miner.voltage < 4) {
      alerts.push({ message: `Voltage Low`, severity: 'warning' });
    }
    if (
      miner.fan_speeds !== undefined &&
      miner.fan_speeds.some((s) => s < 1000)
    ) {
      alerts.push({ message: `Fan Speed Low`, severity: 'warning' });
    }
    return alerts;
  };

  const fetchMinerData = async (ip: string): Promise<Miner | null> => {
    try {
      const response = await fetch(
        `${API_URLS.MINER_DEVICE_URL}/api/miners/live?ip=${ip}`,
        {
          headers: { Accept: 'application/json' },
        }
      );

      const responseData = await response.json();

      if (!response.ok || !responseData.success) {
        console.error(`Failed to fetch data for ${ip}:`, responseData.error);
        return null;
      }

      const data = responseData.data;

      const miner: Miner = {
        id: data.mac || ip + '_' + Date.now(),
        ip: data.ip || ip,
        hostname: data.hostname || 'Unknown',
        mac: data.mac || 'Unknown',
        make: data.make || 'Unknown',
        model: data.model || 'Unknown',
        firmware: data.firmware || 'Unknown',

        status: determineStatus(data),
        is_mining: data.is_mining || false,
        uptime: data.uptime || 0,
        errors: data.errors || [],
        alerts: 0,
        lastSeen: new Date().toLocaleTimeString(),

        hashrate_current: data.hashrate_current || 0,
        hashrate_avg: data.hashrate_avg || 0,
        expected_hashrate: data.expected_hashrate || 0,

        temperature: data.temperature || 0,
        temperature_max: data.temperature_max || 0,
        vr_temperature: data.vr_temperature || 0,

        power_usage: data.power_usage || 0,
        power_limit: data.power_limit || 0,
        efficiency: data.efficiency || 0,
        voltage: data.voltage || 0,

        fan_speeds: data.fan_speeds || [],
        chip_count: data.chip_count || 0,

        primary_pool: data.primary_pool || 'No Pool',
        pools: data.pools || [],
      };

      return miner;
    } catch (err) {
      console.error(`Error fetching data for ${ip}:`, err);
      return null;
    }
  };

  const refreshAllMiners = async () => {
    const currentMiners = minersRef.current;
    if (currentMiners.length === 0) return;

    setLoading(true);

    const results = await Promise.all(
      currentMiners.map((m) => fetchMinerData(m.ip).catch(() => null))
    );

    const updatedMiners: Miner[] = results.map((updated, idx) => {
      const orig = currentMiners[idx];
      if (updated) {
        updated.id = orig.id;
        return updated;
      }
      return {
        ...orig,
        status: 'offline' as const,
        is_mining: false,
        lastSeen: new Date().toLocaleTimeString(),
        alerts: 0,
        hashrate_current: 0,
        hashrate_avg: 0,
        expected_hashrate: 0,
        temperature: 0,
        temperature_max: 0,
        vr_temperature: 0,
        power_usage: 0,
        efficiency: 0,
        voltage: 0,
      };
    });

    setMiners(updatedMiners);
    if (updatedMiners.some((miner) => miner.status === 'online')) {
      setLastUpdate(new Date());
    }
    setLoading(false);
  };

  useEffect(() => {
    if (!autoRefresh || miners.length === 0) return;

    const interval = setInterval(refreshAllMiners, refreshInterval * 1000);
    return () => clearInterval(interval);
  }, [autoRefresh, refreshInterval, miners.length]);

  const addMinerByIP = async () => {
    if (!newMinerIP.trim()) {
      setError('Please enter a valid IP address');
      return;
    }

    setLoading(true);
    setError(null);

    const newMiner = await fetchMinerData(newMinerIP.trim());

    if (newMiner) {
      setMiners((prev) => {
        const exists = prev.find((m) => m.ip === newMiner.ip);
        if (exists) {
          return prev.map((m) =>
            m.ip === newMiner.ip ? { ...newMiner, id: m.id } : m
          );
        }
        return [...prev, newMiner];
      });
      setNewMinerIP('');
      setLastUpdate(new Date());
    } else {
      const errorMsg = `Could not connect to miner at ${newMinerIP.trim()}`;
      setError(errorMsg);
    }

    setLoading(false);
  };

  const handleSearch = () => setSearchQuery(searchInput.trim());
  const clearSearch = () => {
    setSearchInput('');
    setSearchQuery('');
  };

  const totalMiners = miners.length;
  const onlineMiners = miners.filter((m) => m.status === 'online').length;
  const warningMiners = miners.filter((m) => m.status === 'warning').length;
  const offlineMiners = miners.filter((m) => m.status === 'offline').length;
  const totalHashrate = miners.reduce(
    (sum, m) =>
      m.status === 'online' || m.status === 'warning'
        ? sum + (m.hashrate_current || 0)
        : sum,
    0
  );
  const totalPower = miners.reduce(
    (sum, m) =>
      m.status === 'online' || m.status === 'warning'
        ? sum + (m.power_usage || 0)
        : sum,
    0
  );
  const activeMiners = miners.filter(
    (m) => m.status === 'online' || m.status === 'warning'
  );

  const avgEfficiency =
    activeMiners.length > 0
      ? (activeMiners.reduce((sum, m) => sum + (m.efficiency || 0), 0) /
          activeMiners.length) *
        1000
      : 0;

  const allAlerts = miners.flatMap((m) => getAlerts(m));
  const criticalAlerts = allAlerts.filter(
    (a) => a.severity === 'critical'
  ).length;
  const warningAlerts = allAlerts.filter(
    (a) => a.severity === 'warning'
  ).length;
  const totalAlerts = criticalAlerts + warningAlerts;
  const hasAlerts = totalAlerts > 0;

  const displayedMiners =
    !searchQuery || searchQuery.length === 0
      ? miners
      : miners.filter((m) => {
          const q = searchQuery.toLowerCase();
          return (
            (m.ip || '').toLowerCase().includes(q) ||
            (m.hostname || '').toLowerCase().includes(q)
          );
        });

  // Apply status filter
  const filteredByStatus =
    statusFilter === 'all'
      ? displayedMiners
      : displayedMiners.filter((m) => m.status === statusFilter);

  // Apply sorting to the filtered list
  const sortedDisplayedMiners = (() => {
    const arr = [...filteredByStatus];
    if (sortBy === 'all') return arr;

    arr.sort((a, b) => {
      switch (sortBy) {
        case 'efficiency':
          return (b.efficiency || 0) - (a.efficiency || 0);
        case 'hashrate':
          return (b.hashrate_current || 0) - (a.hashrate_current || 0);
        case 'power':
          return (b.power_usage || 0) - (a.power_usage || 0);
        case 'temperature':
          return (b.temperature || 0) - (a.temperature || 0);
        default:
          return 0;
      }
    });

    return arr;
  })();

  const statusStyles: Record<Miner['status'], string> = {
    online: 'bg-emerald-500/10 text-emerald-300 border-emerald-500/40',
    warning: 'bg-amber-500/10 text-amber-300 border-amber-500/40',
    offline: 'bg-rose-500/10 text-rose-300 border-rose-500/40',
  };

  return (
    <div className="min-h-screen w-full px-4 py-6 sm:px-6 lg:px-8">
      <div className="max-w-7xl mx-auto">
        <div className="text-center mb-8">
          <h1 className="text-3xl font-bold text-white mb-4">
            Mining Dashboard
          </h1>

          {error && (
            <div className="text-red-400 border border-red-500 px-4 py-3 rounded max-w-md mx-auto mb-4">
              <strong className="font-bold">Error: </strong>
              <span className="block sm:inline">{error}</span>
            </div>
          )}

          <div className="mt-6 relative flex items-center w-full gap-2 mb-8">
            <div className="absolute left-1/2 transform -translate-x-1/2 flex gap-2">
              <input
                type="text"
                value={newMinerIP}
                onChange={(e) => setNewMinerIP(e.target.value)}
                placeholder="Enter Miner IP"
                aria-label="Miner IP"
                className="w-64 px-3 py-2 text-sm text-center border border-gray-600 bg-gray-800 rounded text-white placeholder-gray-400 focus:outline-none focus:ring-1 focus:ring-gray-500"
                onKeyDown={(e) => e.key === 'Enter' && addMinerByIP()}
              />
              <button
                onClick={addMinerByIP}
                disabled={loading}
                className="px-4 py-2 text-sm text-white rounded bg-gray-800 cursor-pointer"
              >
                {loading ? 'Adding...' : 'Add Miner'}
              </button>
            </div>

            <div className="ml-auto flex items-center gap-2 text-sm text-gray-400">
              <div>
                {lastUpdate
                  ? `Last update: ${lastUpdate.toLocaleString()}`
                  : 'Never updated'}
              </div>
              <button
                onClick={refreshAllMiners}
                className="px-3 py-1 text-sm rounded border border-gray-600 text-white bg-gray-800 hover:bg-gray-700 cursor-pointer"
              >
                Refresh
              </button>
            </div>
          </div>

          <div className="grid grid-cols-2 lg:grid-cols-4 gap-5 text-sm max-w-4xl mx-auto">
            <div className="px-2 py-1 rounded-md border border-gray-600  text-gray-400 text-center">
              <div className="font-medium text-sm">Total Miners</div>
              <div className="text-sm text-white mt-1">{totalMiners}</div>
            </div>
            <div className="px-2 py-1 rounded-md border border-gray-600 text-gray-400 text-center">
              <div className="font-medium text-sm">Total Hashrate</div>
              <div className="text-sm text-white mt-1">
                {totalHashrate.toFixed(3)} TH/s
              </div>
            </div>
            <div className="px-2 py-1 rounded-md border border-gray-600 text-gray-400 text-center">
              <div className="font-medium text-sm">Total Power</div>
              <div className="text-sm text-white mt-1">{totalPower}W</div>
            </div>
            <div className="px-2 py-1 rounded-md border border-gray-600 text-gray-400 text-center">
              <div className="font-medium text-sm">Avg Efficiency</div>
              <div className="text-sm text-white mt-1">
                {avgEfficiency.toFixed(1)} W/TH
              </div>
            </div>
          </div>
        </div>

        {miners.length > 0 && (
          <div className="mb-6">
            <AnalyticsCharts fleetHistory={fleetHistory} />
          </div>
        )}

        {miners.length === 0 ? (
          <div className="text-center py-12 text-gray-400">
            <p className="text-xl">No miners found</p>
            <p className="text-md mt-2">
              Add your miner by entering its IP address above
            </p>
          </div>
        ) : (
          <>
            <div className="flex flex-col gap-4 mb-6 sm:flex-row sm:items-center sm:justify-between">
              <div className="flex flex-wrap gap-2">
                <button
                  onClick={() =>
                    setStatusFilter((s) => (s === 'online' ? 'all' : 'online'))
                  }
                  className={
                    'px-4 py-2 rounded-md border transition text-sm ' +
                    (statusFilter === 'online'
                      ? 'border-blue-400 text-white bg-gray-700'
                      : 'border-gray-600 text-gray-400 hover:bg-gray-800')
                  }
                >
                  {onlineMiners} Online
                </button>
                <button
                  onClick={() =>
                    setStatusFilter((s) =>
                      s === 'warning' ? 'all' : 'warning'
                    )
                  }
                  className={
                    'px-4 py-2 rounded-md border transition text-sm ' +
                    (statusFilter === 'warning'
                      ? 'border-yellow-400 text-white bg-gray-700'
                      : 'border-gray-600 text-gray-400 hover:bg-gray-800')
                  }
                >
                  {warningMiners} Warning
                </button>
                <button
                  onClick={() =>
                    setStatusFilter((s) =>
                      s === 'offline' ? 'all' : 'offline'
                    )
                  }
                  className={
                    'px-4 py-2 rounded-md border transition text-sm ' +
                    (statusFilter === 'offline'
                      ? 'border-red-400 text-white bg-gray-700'
                      : 'border-gray-600 text-gray-400 hover:bg-gray-800')
                  }
                >
                  {offlineMiners} Offline
                </button>
              </div>

              <div className="flex flex-col sm:flex-row sm:items-center gap-2">
                <div className="flex gap-2">
                  <input
                    type="text"
                    value={searchInput}
                    onChange={(e) => setSearchInput(e.target.value)}
                    placeholder="Search by IP or name"
                    aria-label="Search miners"
                    className="px-3 py-2 text-sm border border-gray-600 bg-gray-800 rounded text-white placeholder-gray-400 focus:outline-none focus:ring-1 focus:ring-gray-500"
                    onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
                  />
                  <button
                    onClick={clearSearch}
                    className="px-3 py-2 text-sm text-gray-300 rounded border border-gray-600 bg-gray-900 hover:bg-gray-800 transition whitespace-nowrap"
                  >
                    Clear
                  </button>
                </div>

                <select
                  value={sortBy}
                  onChange={(e) =>
                    setSortBy(
                      e.target.value as
                        | 'all'
                        | 'efficiency'
                        | 'hashrate'
                        | 'power'
                        | 'temperature'
                    )
                  }
                  aria-label="Sort miners"
                  className="px-3 py-2 text-sm border border-gray-600 bg-gray-800 rounded text-white focus:outline-none focus:ring-1 focus:ring-gray-500"
                >
                  <option value="all">Sort By</option>
                  <option value="efficiency">Efficiency (W/TH)</option>
                  <option value="hashrate">Hashrate (TH/s)</option>
                  <option value="power">Power (W)</option>
                  <option value="temperature">Temperature (°C)</option>
                </select>
              </div>
            </div>

            {/* Miners Table */}
            {sortedDisplayedMiners.length === 0 ? (
              <div className="text-center py-12 text-gray-400">
                <p className="text-lg">No miners match your search</p>
              </div>
            ) : (
              <div className="w-full overflow-x-auto">
                <div
                  className="min-w-[800px]  rounded-2xl bg-[#1e1e1e] border border-white/10 shadow-md p-4"
                  style={{ borderColor: colors.cardAccentSecondary }}
                >
                  <div
                    className={`grid ${hasAlerts ? 'grid-cols-8' : 'grid-cols-7'} gap-4 px-4 py-3 text-xs uppercase tracking-wide text-gray-400 border-b border-gray-800/60`}
                  >
                    <div>Model</div>
                    <div>Hashrate</div>
                    <div>Efficiency</div>
                    <div>Power</div>
                    <div>Status</div>
                    {hasAlerts && <div>Alerts</div>}
                    <div>Temp</div>
                    <div>Details</div>
                  </div>

                  <div className="rounded-2xl   shadow-md p-4">
                    {sortedDisplayedMiners.map((miner) => (
                      <div
                        key={miner.id}
                        className={`grid ${hasAlerts ? 'grid-cols-8' : 'grid-cols-7'} gap-4 px-4 py-3 text-sm text-gray-200 items-center`}
                      >
                        <div className="font-medium text-white truncate">
                          {miner.hostname ||
                            (miner.make || miner.model
                              ? [miner.make, miner.model]
                                  .filter(Boolean)
                                  .join(' ')
                              : 'Unknown')}
                        </div>
                        <div className="whitespace-nowrap">
                          {(miner.hashrate_current || 0).toFixed(3)} TH/s
                        </div>
                        <div className="whitespace-nowrap">
                          {((miner.efficiency || 0) * 1000).toFixed(1)} W/TH
                        </div>
                        <div className="whitespace-nowrap">
                          {miner.power_usage || 0} W
                        </div>
                        <div>
                          <span
                            className={`px-2 py-0.5 text-xs rounded border whitespace-nowrap ${statusStyles[miner.status]}`}
                          >
                            {miner.status.toUpperCase()}
                          </span>
                        </div>
                        {hasAlerts &&
                          (() => {
                            const alerts = getAlerts(miner);
                            if (alerts.length === 0) return <div></div>;

                            const isExpanded =
                              expandedAlerts[miner.id] || false;
                            const firstAlert = alerts[0];
                            const remainingCount = alerts.length - 1;
                            return (
                              <div className="flex flex-col gap-1.5">
                                <div
                                  className="cursor-pointer select-none"
                                  onClick={() =>
                                    setExpandedAlerts((prev) => ({
                                      ...prev,
                                      [miner.id]: !prev[miner.id],
                                    }))
                                  }
                                >
                                  <div
                                    className="flex items-center gap-2 px-2.5 py-1.5 rounded-md border text-xs font-medium transition-colors
                                    bg-gray-800/60 border-gray-700/40 text-amber-300 hover:bg-gray-800/80"
                                  >
                                    <span>{firstAlert.message}</span>
                                    {remainingCount > 0 && (
                                      <span className="px-1.5 py-0.5 rounded bg-gray-700/50 text-gray-400 text-[10px]">
                                        +{remainingCount}
                                      </span>
                                    )}
                                    <span className="ml-auto text-gray-500 text-[10px]">
                                      {isExpanded ? '▲' : '▼'}
                                    </span>
                                  </div>
                                </div>

                                {isExpanded && alerts.length > 1 && (
                                  <div className="flex flex-col gap-1 pl-2 border-l-2 text-amber-300 border-gray-700/50">
                                    {alerts.slice(1).map((alert, idx) => (
                                      <div key={idx}>{alert.message}</div>
                                    ))}
                                  </div>
                                )}
                              </div>
                            );
                          })()}

                        <div className="text-gray-300 whitespace-nowrap">
                          {miner.temperature || 0}°C /{' '}
                          {miner.vr_temperature || 0}°C
                        </div>
                        <div className="text-gray-300 whitespace-nowrap">
                          <Link
                            to="#"
                            onClick={(e) => e.preventDefault()}
                            className={`inline-flex px-3 py-1 text-xs rounded border ${
                              minerHistory[miner.id] &&
                              minerHistory[miner.id].length > 1
                                ? 'border-gray-600 bg-gray-800 hover:bg-gray-700 cursor-pointer'
                                : 'border-gray-700 bg-gray-900 text-gray-500 cursor-not-allowed'
                            }`}
                            title={
                              minerHistory[miner.id] &&
                              minerHistory[miner.id].length > 1
                                ? 'View detailed analytics'
                                : 'Collecting data... Please wait'
                            }
                          >
                            Details
                          </Link>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
};

export default MinerInventoryDashboard;
