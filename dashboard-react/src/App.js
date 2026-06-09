import React, { useState } from 'react';
import { BrowserRouter as Router, Routes, Route } from 'react-router-dom';
import { Layout, Menu, theme, ConfigProvider, App as AntApp } from 'antd';
import {
  DashboardOutlined,
  DesktopOutlined,
  AppstoreOutlined,
  GatewayOutlined,
  MonitorOutlined,
  NodeIndexOutlined,
  SettingOutlined,
  ConsoleSqlOutlined,
} from '@ant-design/icons';
import Dashboard from './components/Dashboard';
import DeviceManagement from './components/DeviceManagement';
import ApplicationManagement from './components/ApplicationManagement';
import GatewayManagement from './components/GatewayManagement';
import Monitoring from './components/Monitoring';
import NetworkTopology from './components/NetworkTopology';
import InitialConfiguration from './components/InitialConfiguration';
import Terminal from './components/Terminal';
import './App.css';

const { Header, Sider, Content } = Layout;

function App() {
  const [collapsed, setCollapsed] = useState(false);
  const {
    token: { colorBgContainer, borderRadiusLG },
  } = theme.useToken();

  const menuItems = [
    {
      key: 'dashboard',
      icon: <DashboardOutlined />,
      label: 'Dashboard',
    },
    {
      key: 'initial-config',
      icon: <SettingOutlined />,
      label: 'Initial Configuration',
    },
    {
      key: 'topology',
      icon: <NodeIndexOutlined />,
      label: 'Network Topology',
    },
    {
      key: 'devices',
      icon: <DesktopOutlined />,
      label: 'Device Management',
    },
    {
      key: 'applications',
      icon: <AppstoreOutlined />,
      label: 'Application Management',
    },
    {
      key: 'gateways',
      icon: <GatewayOutlined />,
      label: 'Gateway Management',
    },
    {
      key: 'monitoring',
      icon: <MonitorOutlined />,
      label: 'Monitoring',
    },
    {
      key: 'terminal',
      icon: <ConsoleSqlOutlined />,
      label: 'Terminal',
    },
  ];

  return (
    <ConfigProvider
      theme={{
        token: {
          colorPrimary: '#1890ff',
          borderRadius: 12,
          colorBgContainer: '#ffffff',
          colorBgElevated: '#ffffff',
          colorBgLayout: '#f8fafc',
          colorBorder: '#e2e8f0',
          colorText: '#1e293b',
          colorTextSecondary: '#475569',
          colorTextTertiary: '#64748b',
          colorBgBase: '#ffffff',
          colorSuccess: '#10b981',
          colorWarning: '#f59e0b',
          colorError: '#ef4444',
          colorInfo: '#3b82f6',
          fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, "Noto Sans", sans-serif, "Apple Color Emoji", "Segoe UI Emoji", "Segoe UI Symbol", "Noto Color Emoji"',
          fontSize: 14,
          fontSizeHeading1: 40,
          fontSizeHeading2: 32,
          fontSizeHeading3: 26,
          fontSizeHeading4: 22,
          fontSizeHeading5: 18,
          lineHeight: 1.6,
          boxShadow: '0 10px 25px -5px rgba(0, 0, 0, 0.1), 0 4px 6px -2px rgba(0, 0, 0, 0.05)',
          boxShadowSecondary: '0 4px 12px 0 rgba(0, 0, 0, 0.08), 0 2px 4px 0 rgba(0, 0, 0, 0.06)',
          boxShadowTertiary: '0 1px 3px 0 rgba(0, 0, 0, 0.1), 0 1px 2px 0 rgba(0, 0, 0, 0.06)',
        },
        components: {
          Layout: {
            headerBg: '#ffffff',
            siderBg: '#0f172a',
            bodyBg: '#f8fafc',
            headerHeight: 64,
            headerPadding: '0 24px',
            siderWidth: 240,
            siderCollapsedWidth: 80,
          },
          Menu: {
            itemBg: 'transparent',
            itemSelectedBg: 'rgba(59, 130, 246, 0.1)',
            itemHoverBg: 'rgba(255, 255, 255, 0.05)',
            colorText: '#e2e8f0',
            colorTextSecondary: '#cbd5e1',
            colorTextTertiary: '#94a3b8',
            colorBgContainer: '#0f172a',
            itemSelectedColor: '#3b82f6',
            itemHoverColor: '#ffffff',
            itemActiveColor: '#3b82f6',
            subMenuItemBg: 'transparent',
            itemBorderRadius: 8,
            itemMarginInline: 8,
            itemPaddingInline: 16,
            horizontalItemSelectedColor: '#3b82f6',
            horizontalItemSelectedBg: 'rgba(59, 130, 246, 0.1)',
          },
          Card: {
            borderRadius: 8,
            colorBgContainer: '#ffffff',
            colorBorder: '#e2e8f0',
            boxShadow: '0 2px 8px 0 rgba(0, 0, 0, 0.06), 0 1px 2px 0 rgba(0, 0, 0, 0.04)',
            boxShadowTertiary: '0 1px 2px 0 rgba(0, 0, 0, 0.08), 0 1px 1px 0 rgba(0, 0, 0, 0.04)',
            paddingLG: 16,
            paddingMD: 12,
            paddingSM: 8,
            height: 'auto',
            minHeight: 'auto',
          },
          Table: {
            borderRadius: 12,
            headerBg: '#f8fafc',
            colorBgContainer: '#ffffff',
            colorText: '#1e293b',
            colorTextHeading: '#1e293b',
            colorBorder: '#e2e8f0',
            colorSplit: '#e2e8f0',
            headerColor: '#1e293b',
            headerSortActiveBg: '#f1f5f9',
            headerSortHoverBg: '#e2e8f0',
            rowHoverBg: '#f8fafc',
            rowSelectedBg: 'rgba(59, 130, 246, 0.1)',
            rowSelectedHoverBg: 'rgba(59, 130, 246, 0.15)',
          },
          Button: {
            colorBgContainer: '#ffffff',
            colorBorder: '#e2e8f0',
            colorText: '#1e293b',
            colorPrimary: '#3b82f6',
            colorPrimaryHover: '#2563eb',
            colorPrimaryActive: '#1d4ed8',
            borderRadius: 6,
            controlHeight: 28,
            controlHeightLG: 32,
            controlHeightSM: 22,
            paddingInline: 12,
            paddingInlineLG: 16,
            paddingInlineSM: 8,
            boxShadow: '0 1px 2px 0 rgba(0, 0, 0, 0.05)',
            boxShadowSecondary: '0 1px 2px 0 rgba(0, 0, 0, 0.05)',
            height: 'auto',
            minHeight: 'auto',
          },
          Input: {
            colorBgContainer: '#ffffff',
            colorBorder: '#d9d9d9',
            colorText: '#262626',
            colorTextPlaceholder: '#bfbfbf',
            borderRadius: 4,
            controlHeight: 28,
            controlHeightLG: 32,
            controlHeightSM: 22,
            paddingInline: 8,
            paddingInlineLG: 12,
            paddingInlineSM: 6,
            hoverBorderColor: '#40a9ff',
            activeBorderColor: '#1890ff',
            activeShadow: '0 0 0 2px rgba(24, 144, 255, 0.2)',
            height: 'auto',
            minHeight: 'auto',
          },
          Select: {
            colorBgContainer: '#ffffff',
            colorBorder: '#d9d9d9',
            colorText: '#262626',
            colorTextPlaceholder: '#bfbfbf',
            borderRadius: 4,
            controlHeight: 28,
            controlHeightLG: 32,
            controlHeightSM: 22,
            optionSelectedBg: '#e6f7ff',
            optionActiveBg: '#f5f5f5',
            optionPadding: '4px 8px',
            height: 'auto',
            minHeight: 'auto',
          },
          Modal: {
            colorBgElevated: '#ffffff',
            colorText: '#262626',
            borderRadius: 8,
            paddingLG: 24,
            paddingMD: 20,
            paddingSM: 16,
            boxShadow: '0 6px 16px 0 rgba(0, 0, 0, 0.08), 0 3px 6px -4px rgba(0, 0, 0, 0.12), 0 9px 28px 8px rgba(0, 0, 0, 0.05)',
          },
          Form: {
            labelColor: '#262626',
            labelRequiredMarkColor: '#ff4d4f',
            labelFontSize: 13,
            labelHeight: 24,
            itemMarginBottom: 16,
            verticalLabelPadding: '0 0 4px',
            verticalLabelMargin: 0,
          },
          Typography: {
            colorText: '#262626',
            colorTextSecondary: '#595959',
            colorTextTertiary: '#8c8c8c',
            colorTextQuaternary: '#bfbfbf',
            colorLink: '#1890ff',
            colorLinkHover: '#40a9ff',
            colorLinkActive: '#096dd9',
            colorSuccess: '#52c41a',
            colorWarning: '#faad14',
            colorError: '#ff4d4f',
            colorInfo: '#1890ff',
          },
          Tag: {
            colorText: '#262626',
            colorTextLightSolid: '#ffffff',
            colorBg: '#f5f5f5',
            colorSuccess: '#52c41a',
            colorWarning: '#faad14',
            colorError: '#ff4d4f',
            colorInfo: '#1890ff',
            borderRadius: 4,
            fontSizeSM: 11,
            lineHeightSM: 16,
            paddingInlineSM: 6,
            margin: 0,
            height: 'auto',
            minHeight: 'auto',
          },
          Tooltip: {
            colorBgSpotlight: '#ffffff',
            colorTextLightSolid: '#262626',
            borderRadius: 6,
            boxShadow: '0 6px 16px 0 rgba(0, 0, 0, 0.08), 0 3px 6px -4px rgba(0, 0, 0, 0.12), 0 9px 28px 8px rgba(0, 0, 0, 0.05)',
          },
          Popconfirm: {
            colorBgElevated: '#ffffff',
            colorText: '#262626',
            borderRadius: 8,
            boxShadow: '0 6px 16px 0 rgba(0, 0, 0, 0.08), 0 3px 6px -4px rgba(0, 0, 0, 0.12), 0 9px 28px 8px rgba(0, 0, 0, 0.05)',
          },
          Steps: {
            colorText: '#262626',
            colorTextDescription: '#8c8c8c',
            colorPrimary: '#1890ff',
            colorSuccess: '#52c41a',
            colorWarning: '#faad14',
            colorError: '#ff4d4f',
            colorInfo: '#1890ff',
            colorTextDisabled: '#bfbfbf',
            colorBgContainer: '#ffffff',
            colorBorder: '#d9d9d9',
            colorSplit: '#e8e8e8',
            dotSize: 8,
            dotCurrentSize: 10,
            titleLineHeight: 32,
            descriptionMaxWidth: 140,
            customIconSize: 32,
            customIconTop: 0,
            customIconFontSize: 16,
            iconSize: 32,
            iconTop: 0,
            iconFontSize: 16,
            iconMargin: '0 8px 0 0',
            iconSizeSM: 24,
            iconFontSizeSM: 12,
            iconMarginSM: '0 4px 0 0',
            progressDotSize: 8,
            progressDotBorderWidth: 2,
            progressDotBorderColor: '#d9d9d9',
            progressDotActiveBorderColor: '#1890ff',
            progressDotActiveBg: '#1890ff',
            progressDotBg: '#d9d9d9',
            progressDotColor: '#ffffff',
            progressDotColorActive: '#ffffff',
            progressDotColorError: '#ffffff',
            progressDotColorWarning: '#ffffff',
            progressDotColorInfo: '#ffffff',
            progressDotColorSuccess: '#ffffff',
            progressDotColorDisabled: '#bfbfbf',
            progressDotColorDisabledActive: '#bfbfbf',
            progressDotColorDisabledError: '#bfbfbf',
            progressDotColorDisabledWarning: '#bfbfbf',
            progressDotColorDisabledInfo: '#bfbfbf',
            progressDotColorDisabledSuccess: '#bfbfbf',
          },
        },
      }}
    >
      <AntApp>
        <Router future={{ v7_startTransition: true, v7_relativeSplatPath: true }}>
        <Layout style={{ minHeight: '100vh' }}>
          <Sider 
            trigger={null} 
            collapsible 
            collapsed={collapsed}
            style={{
              background: '#0f172a',
              boxShadow: '4px 0 12px 0 rgba(0, 0, 0, 0.1)',
            }}
          >
            <div className="logo" style={{ 
              height: 100, 
              margin: 16, 
              background: '#ffffff',
              borderRadius: 12,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexDirection: 'column',
              padding: '12px',
              boxShadow: '0 4px 12px 0 rgba(0, 0, 0, 0.1)',
              border: '1px solid #e2e8f0'
            }}>
              <img 
                src="/Logo_Serics.png" 
                alt="MdsLab Logo" 
                style={{ 
                  height: collapsed ? 32 : 48, 
                  width: 'auto',
                  marginBottom: collapsed ? 0 : 8
                }} 
              />
              {!collapsed && (
                <div style={{ 
                  fontSize: '12px', 
                  fontWeight: 'bold', 
                  color: '#262626',
                  textAlign: 'center',
                  lineHeight: 1.2
                }}>
                  RETROSPECT
                  <br />
                  <span style={{ fontSize: '9px', color: '#8c8c8c' }}>
                    PROSODIAC - UniMe
                  </span>
                </div>
              )}
            </div>
            <Menu
              theme="dark"
              mode="inline"
              defaultSelectedKeys={['dashboard']}
              items={menuItems}
              onClick={({ key }) => {
                // Navigation will be handled by React Router
                window.history.pushState({}, '', `/${key}`);
                window.dispatchEvent(new PopStateEvent('popstate'));
              }}
              style={{
                background: 'transparent',
                border: 'none',
              }}
            />
          </Sider>
          <Layout>
            <Header
              style={{
                padding: '0 24px',
                background: colorBgContainer,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                height: '64px',
                minHeight: '64px',
                borderBottom: '1px solid #e2e8f0',
                boxShadow: '0 4px 12px 0 rgba(0, 0, 0, 0.08)',
                position: 'sticky',
                top: 0,
                zIndex: 1000,
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: '16px' }}>
                <div style={{ 
                  color: '#262626', 
                  fontSize: '20px', 
                  fontWeight: 'bold'
                }}>
                  RETROSPECT Dashboard
                </div>
                <div style={{ 
                  color: '#8c8c8c', 
                  fontSize: '12px',
                  fontStyle: 'italic'
                }}>
                  "secuRE inTegration middlewaRe fOr cpS comPutE ConTinuum"
                </div>
              </div>
            </Header>
            <Content
              style={{
                margin: '24px 16px',
                padding: 24,
                minHeight: 280,
                background: colorBgContainer,
                borderRadius: 16,
                boxShadow: '0 4px 12px 0 rgba(0, 0, 0, 0.08), 0 2px 4px 0 rgba(0, 0, 0, 0.06)',
              }}
            >
              <Routes>
                <Route path="/" element={<Dashboard />} />
                <Route path="/dashboard" element={<Dashboard />} />
                <Route path="/initial-config" element={<InitialConfiguration />} />
                <Route path="/topology" element={<NetworkTopology />} />
                <Route path="/devices" element={<DeviceManagement />} />
                <Route path="/applications" element={<ApplicationManagement />} />
                <Route path="/gateways" element={<GatewayManagement />} />
                <Route path="/monitoring" element={<Monitoring />} />
                <Route path="/terminal" element={<Terminal />} />
              </Routes>
            </Content>
          </Layout>
        </Layout>
        </Router>
      </AntApp>
    </ConfigProvider>
  );
}

export default App;
